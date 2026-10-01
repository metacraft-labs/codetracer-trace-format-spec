//! Generates PolkaVM blobs (no RISC-V toolchain) for the trace-format corpus.
use polkavm_common::program::{asm, Instruction, InstructionSetKind, Reg::*};
use polkavm_common::writer::ProgramBlobBuilder;

fn blob(code: &[Instruction], isa: InstructionSetKind) -> Vec<u8> {
    let mut b = ProgramBlobBuilder::new(isa);
    b.set_stack_size(4096);
    b.set_rw_data_size(64);
    b.add_export_by_basic_block(0, b"main");
    b.set_code(code, &[]);
    b.into_vec().expect("blob")
}

fn counted_loop(n: u32) -> Vec<Instruction> {
    vec![
        // BB0
        asm::load_imm(A0, 0),
        asm::load_imm(A1, n),
        asm::load_imm(S0, 0),
        asm::jump(1),
        // BB1 header
        asm::branch_greater_or_equal_unsigned(A0, A1, 3),
        // BB2 body: S0 += A0 * 3 ; A0 += 1
        asm::add_32(S1, A0, A0),
        asm::add_32(S1, S1, A0),
        asm::add_32(S0, S0, S1),
        asm::add_imm_32(A0, A0, 1),
        asm::jump(1),
        // BB3 exit
        asm::move_reg(A0, S0),
        asm::ret(),
    ]
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    let l32 = InstructionSetKind::Latest32;
    let l64 = InstructionSetKind::Latest64;
    let progs: Vec<(&str, Vec<u8>)> = vec![
        ("add", blob(&[asm::load_imm(A0, 10), asm::load_imm(A1, 32), asm::add_32(A0, A0, A1), asm::ret()], l32)),
        ("branching", blob(&[
            asm::load_imm(A0, 5),
            asm::branch_greater_or_equal_unsigned_imm(A0, 10, 2),
            asm::load_imm(S0, 111), asm::ret(),
            asm::load_imm(S0, 222), asm::ret(),
        ], l32)),
        ("loop4", blob(&counted_loop(4), l32)),
        ("loop200", blob(&counted_loop(200), l32)),
        ("loop5000", blob(&counted_loop(5000), l32)),
        ("ecalli_host_calls", blob(&[
            asm::load_imm(A0, 1), asm::ecalli(5),
            asm::load_imm(A0, 2), asm::ecalli(6),
            asm::load_imm(A0, 3), asm::ecalli(28),
            asm::load_imm(A0, 4), asm::ecalli(2),
            asm::load_imm(A0, 99), asm::ret(),
        ], l32)),
        ("sum4_move", blob(&[
            asm::load_imm(A0, 1), asm::load_imm(A1, 2), asm::load_imm(A2, 3), asm::load_imm(A3, 4),
            asm::add_32(S0, A0, A1), asm::add_32(S0, S0, A2), asm::add_32(S0, S0, A3),
            asm::move_reg(A0, S0), asm::ret(),
        ], l32)),
        ("arith64", blob(&[
            asm::load_imm(A0, 100000), asm::load_imm(A1, 300000),
            asm::mul_64(A2, A0, A1), asm::add_64(A3, A2, A0), asm::add_imm_64(A0, A3, 7),
            asm::ret(),
        ], l64)),
        ("trap", blob(&[asm::load_imm(A0, 42), asm::load_imm(A1, 7), asm::trap(), asm::load_imm(A0, 999), asm::ret()], l32)),
    ];
    for (n, b) in progs {
        std::fs::write(out.join(format!("{n}.polkavm")), b).unwrap();
    }
}
