// Compiler:
//
// Run-time:
//   status: 0

#![feature(asm)]

fn main() {
    unsafe {
        asm!("nop");
    }

    let x: u64;
    unsafe {
        asm!("mov $5, {}",
            out(reg) x,
            options(att_syntax)
        );
    }
    assert_eq!(x, 5);

    let x: u64;
    let input: u64 = 42;
    unsafe {
        asm!("mov {input}, {output}",
             "add $1, {output}",
            input = in(reg) input,
            output = out(reg) x,
            options(att_syntax)
        );
    }
    assert_eq!(x, 43);

    let x: u64;
    unsafe {
        asm!("mov {}, 6",
            out(reg) x,
        );
    }
    assert_eq!(x, 6);

    let x: u64;
    let input: u64 = 42;
    unsafe {
        asm!("mov {output}, {input}",
             "add {output}, 1",
            input = in(reg) input,
            output = out(reg) x,
        );
    }
    assert_eq!(x, 43);
}
