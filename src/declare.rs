use gccjit::{Function, FunctionType, GlobalKind, LValue, RValue, Type};
use rustc_codegen_ssa::traits::BaseTypeMethods;
use rustc_middle::ty::Ty;
use rustc_target::abi::call::FnAbi;

use crate::abi::FnAbiGccExt;
use crate::context::{CodegenCx, unit_name};

impl<'gcc, 'tcx> CodegenCx<'gcc, 'tcx> {
    pub fn get_or_insert_global(&self, name: &str, ty: Type<'gcc>) -> RValue<'gcc> {
        if self.globals.borrow().contains_key(name) {
            let typ = self.globals.borrow().get(name).expect("global").get_type();
            self.context.new_global(None, GlobalKind::Imported, typ, name)
                .get_address(None)
        }
        else {
            self.declare_global(name, ty)
        }
    }

    pub fn declare_unnamed_global(&self, ty: Type<'gcc>) -> LValue<'gcc> {
        let index = self.global_gen_sym_counter.get();
        self.global_gen_sym_counter.set(index + 1);
        let name = format!("global_{}_{}", index, unit_name(&self.codegen_unit));
        self.context.new_global(None, GlobalKind::Exported, ty, &name)
    }

    pub fn declare_global_with_linkage(&self, name: &str, ty: Type<'gcc>, linkage: GlobalKind) -> RValue<'gcc> {
        //debug!("declare_global_with_linkage(name={:?})", name);
        let global = self.context.new_global(None, linkage, ty, name)
            .get_address(None);
        self.globals.borrow_mut().insert(name.to_string(), global);
        // NOTE: global seems to only be global in a module. So save the name instead of the value
        // to import it later.
        self.global_names.borrow_mut().insert(global, name.to_string());
        global
    }

    pub fn declare_func(&self, name: &str, return_type: Type<'gcc>, params: &[Type<'gcc>], variadic: bool) -> RValue<'gcc> {
        self.linkage.set(FunctionType::Exported);
        let func = declare_raw_fn(self, name, () /*llvm::CCallConv*/, return_type, params, variadic);
        // FIXME: this is a wrong cast. That requires changing the compiler API.
        unsafe { std::mem::transmute(func) }
    }

    pub fn declare_global(&self, name: &str, ty: Type<'gcc>) -> RValue<'gcc> {
        //debug!("declare_global(name={:?})", name);
        let global = self.context.new_global(None, GlobalKind::Exported, ty, name)
            .get_address(None);
        self.globals.borrow_mut().insert(name.to_string(), global);
        // NOTE: global seems to only be global in a module. So save the name instead of the value
        // to import it later.
        self.global_names.borrow_mut().insert(global, name.to_string());
        global
    }

    pub fn declare_cfn(&self, name: &str, fn_type: Type<'gcc>) -> RValue<'gcc> {
        // TODO: use the fn_type parameter.
        let const_string = self.context.new_type::<u8>().make_pointer().make_pointer();
        let return_type = self.type_i32();
        let variadic = false;
        self.linkage.set(FunctionType::Exported);
        let func = declare_raw_fn(self, name, () /*llvm::CCallConv*/, return_type, &[self.type_i32(), const_string], variadic);
        // NOTE: it is needed to set the current_func here as well, because get_fn() is not called
        // for the main function.
        *self.current_func.borrow_mut() = Some(func);
        // FIXME: this is a wrong cast. That requires changing the compiler API.
        unsafe { std::mem::transmute(func) }
    }

    pub fn declare_fn(&self, name: &str, fn_abi: &FnAbi<'tcx, Ty<'tcx>>) -> RValue<'gcc> {
        //debug!("declare_rust_fn(name={:?}, fn_abi={:?})", name, fn_abi);
        let (return_type, params, variadic) = fn_abi.gcc_type(self);
        let func = declare_raw_fn(self, name, () /*fn_abi.llvm_cconv()*/, return_type, &params, variadic);
        //fn_abi.apply_attrs_llfn(self, func);
        // FIXME: this is a wrong cast. That requires changing the compiler API.
        unsafe { std::mem::transmute(func) }
    }

    pub fn define_global(&self, name: &str, ty: Type<'gcc>) -> Option<RValue<'gcc>> {
        Some(self.get_or_insert_global(name, ty))
    }

    pub fn define_private_global(&self, ty: Type<'gcc>) -> RValue<'gcc> {
        let global = self.declare_unnamed_global(ty);
        global.get_address(None)
    }

    pub fn get_declared_value(&self, name: &str) -> Option<RValue<'gcc>> {
        //debug!("get_declared_value(name={:?})", name);
        // TODO: use a different field than globals, because this seems to return a function?
        self.globals.borrow().get(name).cloned()
    }

    fn get_defined_value(&self, name: &str) -> Option<RValue<'gcc>> {
        // TODO: gcc does not allow global initialization.
        None
        /*self.get_declared_value(name).and_then(|val| {
            let declaration = unsafe { llvm::LLVMIsDeclaration(val) != 0 };
            if !declaration { Some(val) } else { None }
        })*/
    }
}

/// Declare a function.
///
/// If there’s a value with the same name already declared, the function will
/// update the declaration and return existing Value instead.
fn declare_raw_fn<'gcc>(cx: &CodegenCx<'gcc, '_>, name: &str, callconv: () /*llvm::CallConv*/, return_type: Type<'gcc>, param_types: &[Type<'gcc>], variadic: bool) -> Function<'gcc> {
    //debug!("declare_raw_fn(name={:?}, ty={:?})", name, ty);
    /*let llfn = unsafe {
        llvm::LLVMRustGetOrInsertFunction(cx.llmod, name.as_ptr().cast(), name.len(), ty)
    };*/

    if name == "llvm.x86.xgetbv" {
        // TODO: support other LLVM intrinsics.
        let func = cx.context.get_builtin_function("__builtin_trap");
        cx.functions.borrow_mut().insert(name.to_string(), func);
        return func;
    }
    let func =
        if cx.functions.borrow().contains_key(name) {
            *cx.functions.borrow().get(name).expect("function")
        }
        else {
            let params: Vec<_> = param_types.into_iter().enumerate()
                .map(|(index, param)| cx.context.new_parameter(None, *param, &format!("param{}", index))) // TODO: set name.
                .collect();
            //println!("Func {}({:?})", name, param_types);
            let func = cx.context.new_function(None, cx.linkage.get(), return_type, &params, mangle_name(name), variadic);
            cx.functions.borrow_mut().insert(name.to_string(), func);
            func
        };

    //llvm::SetFunctionCallConv(llfn, callconv); // TODO
    // Function addresses in Rust are never significant, allowing functions to
    // be merged.
    //llvm::SetUnnamedAddress(llfn, llvm::UnnamedAddr::Global); // TODO

    /*if cx.tcx.sess.opts.cg.no_redzone.unwrap_or(cx.tcx.sess.target.target.options.disable_redzone) {
        llvm::Attribute::NoRedZone.apply_llfn(Function, llfn);
    }*/

    //attributes::default_optimisation_attrs(cx.tcx.sess, llfn);
    //attributes::non_lazy_bind(cx.sess(), llfn);

    // FIXME: invalid cast.
    cx.globals.borrow_mut().insert(name.to_string(), unsafe { std::mem::transmute(func) });
    func
}

// FIXME: this is a hack because libgccjit currently only supports alpha, num and _.
// Unsupported characters: `$` and `.`.
pub fn mangle_name(name: &str) -> String {
    name.replace(|char: char| {
        if !char.is_alphanumeric() && char != '_' {
            debug_assert!("$.".contains(char), "Unsupported char in function name: {}", char);
            true
        }
        else {
            false
        }
    }, "_")
}
