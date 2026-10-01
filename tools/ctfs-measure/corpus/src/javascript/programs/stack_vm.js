// A small bytecode compiler + stack VM: a switch-dispatch interpreter loop
// with jumps, calls, frames and a few builtins.
const OP = { PUSH: 0, LOAD: 1, STORE: 2, ADD: 3, SUB: 4, MUL: 5, LT: 6, JMP: 7, JZ: 8, CALL: 9, RET: 10, PRINT: 11, HALT: 12, MOD: 13, EQ: 14, DUP: 15, POP: 16 };

class Assembler {
  constructor() { this.code = []; this.labels = {}; this.fixups = []; }
  emit(op, arg) { this.code.push(op, arg === undefined ? 0 : arg); return this; }
  label(name) { this.labels[name] = this.code.length; return this; }
  jump(op, name) { this.fixups.push([this.code.length + 1, name]); return this.emit(op, -1); }
  finish() {
    for (const [at, name] of this.fixups) {
      if (!(name in this.labels)) throw new Error("undefined label " + name);
      this.code[at] = this.labels[name];
    }
    return Int32Array.from(this.code);
  }
}

function run(code, nlocals, output) {
  const stack = [];
  const frames = [{ ret: -1, locals: new Array(nlocals).fill(0) }];
  let pc = 0;
  let steps = 0;
  for (;;) {
    steps++;
    const op = code[pc], arg = code[pc + 1];
    pc += 2;
    const fr = frames[frames.length - 1];
    switch (op) {
      case OP.PUSH: stack.push(arg); break;
      case OP.LOAD: stack.push(fr.locals[arg]); break;
      case OP.STORE: fr.locals[arg] = stack.pop(); break;
      case OP.ADD: { const b = stack.pop(); stack.push(stack.pop() + b); break; }
      case OP.SUB: { const b = stack.pop(); stack.push(stack.pop() - b); break; }
      case OP.MUL: { const b = stack.pop(); stack.push(stack.pop() * b); break; }
      case OP.MOD: { const b = stack.pop(); stack.push(stack.pop() % b); break; }
      case OP.LT: { const b = stack.pop(); stack.push(stack.pop() < b ? 1 : 0); break; }
      case OP.EQ: { const b = stack.pop(); stack.push(stack.pop() === b ? 1 : 0); break; }
      case OP.DUP: stack.push(stack[stack.length - 1]); break;
      case OP.POP: stack.pop(); break;
      case OP.JMP: pc = arg; break;
      case OP.JZ: if (stack.pop() === 0) pc = arg; break;
      case OP.CALL: {
        const locals = new Array(nlocals).fill(0);
        locals[0] = stack.pop();
        frames.push({ ret: pc, locals });
        pc = arg;
        break;
      }
      case OP.RET: {
        const f = frames.pop();
        pc = f.ret;
        break;
      }
      case OP.PRINT: output.push(stack.pop()); break;
      case OP.HALT: return steps;
      default: throw new Error("bad opcode " + op);
    }
  }
}

// main: for i in 0..N: print(fib(i)); then sum of i%7 over 0..M
function program(N, M) {
  const a = new Assembler();
  a.emit(OP.PUSH, 0).emit(OP.STORE, 1)
    .label("loop").emit(OP.LOAD, 1).emit(OP.PUSH, N).emit(OP.LT).jump(OP.JZ, "done1")
    .emit(OP.LOAD, 1).jump(OP.CALL, "fib").emit(OP.PRINT)
    .emit(OP.LOAD, 1).emit(OP.PUSH, 1).emit(OP.ADD).emit(OP.STORE, 1).jump(OP.JMP, "loop")
    .label("done1")
    .emit(OP.PUSH, 0).emit(OP.STORE, 1).emit(OP.PUSH, 0).emit(OP.STORE, 2)
    .label("loop2").emit(OP.LOAD, 1).emit(OP.PUSH, M).emit(OP.LT).jump(OP.JZ, "done2")
    .emit(OP.LOAD, 2).emit(OP.LOAD, 1).emit(OP.PUSH, 7).emit(OP.MOD).emit(OP.ADD).emit(OP.STORE, 2)
    .emit(OP.LOAD, 1).emit(OP.PUSH, 1).emit(OP.ADD).emit(OP.STORE, 1).jump(OP.JMP, "loop2")
    .label("done2").emit(OP.LOAD, 2).emit(OP.PRINT).emit(OP.HALT)
    // fib(n): if n < 2 return n else fib(n-1)+fib(n-2)
    .label("fib").emit(OP.LOAD, 0).emit(OP.PUSH, 2).emit(OP.LT).jump(OP.JZ, "rec")
    .emit(OP.LOAD, 0).emit(OP.RET)
    .label("rec").emit(OP.LOAD, 0).emit(OP.PUSH, 1).emit(OP.SUB).jump(OP.CALL, "fib")
    .emit(OP.LOAD, 0).emit(OP.PUSH, 2).emit(OP.SUB).jump(OP.CALL, "fib").emit(OP.ADD).emit(OP.RET);
  return a.finish();
}

const N = Number(process.argv[2] || 12);
const out = [];
const steps = run(program(N, 200), 4, out);
console.log("vm steps", steps);
console.log("output", out.join(" "));
