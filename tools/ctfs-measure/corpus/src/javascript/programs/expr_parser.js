// Tokenizer + recursive-descent parser + tree-walking evaluator for a small
// expression language with let-bindings, functions, and conditionals.
class Token {
  constructor(kind, value, pos) {
    this.kind = kind;
    this.value = value;
    this.pos = pos;
  }
}

function tokenize(src) {
  const tokens = [];
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (ch === " " || ch === "\n" || ch === "\t") {
      i++;
      continue;
    }
    if (/[0-9]/.test(ch)) {
      let j = i;
      while (j < src.length && /[0-9.]/.test(src[j])) j++;
      tokens.push(new Token("num", parseFloat(src.slice(i, j)), i));
      i = j;
      continue;
    }
    if (/[A-Za-z_]/.test(ch)) {
      let j = i;
      while (j < src.length && /[A-Za-z0-9_]/.test(src[j])) j++;
      const word = src.slice(i, j);
      const kw = ["let", "in", "if", "then", "else", "fn"].includes(word);
      tokens.push(new Token(kw ? word : "id", word, i));
      i = j;
      continue;
    }
    const two = src.slice(i, i + 2);
    if (["==", "<=", ">=", "!=", "=>"].includes(two)) {
      tokens.push(new Token("op", two, i));
      i += 2;
      continue;
    }
    if ("+-*/%()<>=,".includes(ch)) {
      tokens.push(new Token("op", ch, i));
      i++;
      continue;
    }
    throw new SyntaxError(`unexpected '${ch}' at ${i}`);
  }
  tokens.push(new Token("eof", null, src.length));
  return tokens;
}

class Parser {
  constructor(tokens) {
    this.tokens = tokens;
    this.pos = 0;
  }
  peek() {
    return this.tokens[this.pos];
  }
  next() {
    return this.tokens[this.pos++];
  }
  expect(kind, value) {
    const t = this.next();
    if (t.kind !== kind || (value !== undefined && t.value !== value)) {
      throw new SyntaxError(`expected ${value || kind} at ${t.pos}, got ${t.value}`);
    }
    return t;
  }
  isOp(v) {
    const t = this.peek();
    return t.kind === "op" && t.value === v;
  }
  parseExpr() {
    const t = this.peek();
    if (t.kind === "let") {
      this.next();
      const name = this.expect("id").value;
      this.expect("op", "=");
      const value = this.parseExpr();
      this.expect("in");
      const body = this.parseExpr();
      return { type: "let", name, value, body };
    }
    if (t.kind === "if") {
      this.next();
      const cond = this.parseExpr();
      this.expect("then");
      const yes = this.parseExpr();
      this.expect("else");
      const no = this.parseExpr();
      return { type: "if", cond, yes, no };
    }
    if (t.kind === "fn") {
      this.next();
      this.expect("op", "(");
      const params = [];
      while (!this.isOp(")")) {
        params.push(this.expect("id").value);
        if (this.isOp(",")) this.next();
      }
      this.next();
      this.expect("op", "=>");
      const body = this.parseExpr();
      return { type: "fn", params, body };
    }
    return this.parseCompare();
  }
  parseCompare() {
    let left = this.parseAdd();
    while (["<", ">", "<=", ">=", "==", "!="].some((o) => this.isOp(o))) {
      const op = this.next().value;
      left = { type: "bin", op, left, right: this.parseAdd() };
    }
    return left;
  }
  parseAdd() {
    let left = this.parseMul();
    while (this.isOp("+") || this.isOp("-")) {
      const op = this.next().value;
      left = { type: "bin", op, left, right: this.parseMul() };
    }
    return left;
  }
  parseMul() {
    let left = this.parseCall();
    while (this.isOp("*") || this.isOp("/") || this.isOp("%")) {
      const op = this.next().value;
      left = { type: "bin", op, left, right: this.parseCall() };
    }
    return left;
  }
  parseCall() {
    let callee = this.parseAtom();
    while (this.isOp("(")) {
      this.next();
      const args = [];
      while (!this.isOp(")")) {
        args.push(this.parseExpr());
        if (this.isOp(",")) this.next();
      }
      this.next();
      callee = { type: "call", callee, args };
    }
    return callee;
  }
  parseAtom() {
    const t = this.next();
    if (t.kind === "num") return { type: "num", value: t.value };
    if (t.kind === "id") return { type: "var", name: t.value };
    if (t.kind === "op" && t.value === "(") {
      const e = this.parseExpr();
      this.expect("op", ")");
      return e;
    }
    if (t.kind === "op" && t.value === "-") {
      return { type: "bin", op: "-", left: { type: "num", value: 0 }, right: this.parseAtom() };
    }
    throw new SyntaxError(`unexpected token ${t.value} at ${t.pos}`);
  }
}

class Env {
  constructor(vars, parent) {
    this.vars = vars;
    this.parent = parent;
  }
  lookup(name) {
    let env = this;
    while (env) {
      if (Object.prototype.hasOwnProperty.call(env.vars, name)) return env.vars[name];
      env = env.parent;
    }
    throw new ReferenceError(`unbound ${name}`);
  }
}

function evaluate(node, env) {
  switch (node.type) {
    case "num":
      return node.value;
    case "var":
      return env.lookup(node.name);
    case "let": {
      const vars = {};
      const inner = new Env(vars, env);
      vars[node.name] = evaluate(node.value, inner);
      return evaluate(node.body, inner);
    }
    case "if":
      return evaluate(node.cond, env) ? evaluate(node.yes, env) : evaluate(node.no, env);
    case "fn":
      return { params: node.params, body: node.body, env };
    case "call": {
      const f = evaluate(node.callee, env);
      const args = node.args.map((a) => evaluate(a, env));
      if (typeof f === "function") return f(...args);
      const vars = {};
      f.params.forEach((p, i) => (vars[p] = args[i]));
      return evaluate(f.body, new Env(vars, f.env));
    }
    case "bin": {
      const a = evaluate(node.left, env);
      const b = evaluate(node.right, env);
      switch (node.op) {
        case "+": return a + b;
        case "-": return a - b;
        case "*": return a * b;
        case "/": return a / b;
        case "%": return a % b;
        case "<": return a < b ? 1 : 0;
        case ">": return a > b ? 1 : 0;
        case "<=": return a <= b ? 1 : 0;
        case ">=": return a >= b ? 1 : 0;
        case "==": return a === b ? 1 : 0;
        case "!=": return a !== b ? 1 : 0;
      }
    }
  }
  throw new Error(`bad node ${node.type}`);
}

const globals = new Env({ sqrt: Math.sqrt, max: Math.max, min: Math.min }, null);
const programs = [
  "let fib = fn(n) => if n < 2 then n else fib(n - 1) + fib(n - 2) in fib(14)",
  "let fact = fn(n) => if n == 0 then 1 else n * fact(n - 1) in fact(12)",
  "let gcd = fn(a, b) => if b == 0 then a else gcd(b, a % b) in gcd(1071, 462)",
  "let sum = fn(n, acc) => if n == 0 then acc else sum(n - 1, acc + n * n) in sum(300, 0)",
  "let compose = fn(f, g) => fn(x) => f(g(x)) in let inc = fn(x) => x + 1 in let dbl = fn(x) => x * 2 in compose(inc, dbl)(20)",
  "max(sqrt(144) * 3, min(7, 4) + 30) - (1 + 2) * (3 - 4 / 2)",
  "let ack = fn(m, n) => if m == 0 then n + 1 else if n == 0 then ack(m - 1, 1) else ack(m - 1, ack(m, n - 1)) in ack(2, 3)",
  "let x = 1 in let y = (",
];
for (const src of programs) {
  try {
    const ast = new Parser(tokenize(src)).parseExpr();
    console.log(src.slice(0, 40), "=>", evaluate(ast, globals));
  } catch (e) {
    console.log("error:", e.message);
  }
}
