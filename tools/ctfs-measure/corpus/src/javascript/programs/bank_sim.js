// Object-oriented simulation: classes, inheritance, getters/setters,
// static members, exceptions with try/catch/finally, polymorphic dispatch.
class InsufficientFunds extends Error {
  constructor(account, amount) {
    super(`account ${account.id} cannot cover ${amount}`);
    this.name = "InsufficientFunds";
    this.amount = amount;
  }
}

class Account {
  static nextId = 1;
  constructor(owner, balance = 0) {
    this.id = Account.nextId++;
    this.owner = owner;
    this._balance = balance;
    this.history = [];
  }
  get balance() { return this._balance; }
  set balance(v) {
    if (!Number.isFinite(v)) throw new TypeError("balance must be finite");
    this._balance = v;
  }
  deposit(amount) {
    if (amount <= 0) throw new RangeError("deposit must be positive");
    this.balance = this.balance + amount;
    this.history.push({ kind: "dep", amount });
  }
  withdraw(amount) {
    if (amount > this.available()) throw new InsufficientFunds(this, amount);
    this.balance = this.balance - amount;
    this.history.push({ kind: "wd", amount });
  }
  available() { return this.balance; }
  monthEnd() {}
  toString() { return `${this.constructor.name}#${this.id}(${this.owner}: ${this.balance.toFixed(2)})`; }
}

class Savings extends Account {
  constructor(owner, balance, rate) {
    super(owner, balance);
    this.rate = rate;
  }
  monthEnd() {
    const interest = Math.round(this.balance * this.rate * 100) / 100;
    if (interest > 0) this.deposit(interest);
  }
}

class Checking extends Account {
  constructor(owner, balance, overdraft) {
    super(owner, balance);
    this.overdraft = overdraft;
  }
  available() { return this.balance + this.overdraft; }
  monthEnd() {
    if (this.balance < 0) {
      this.balance = this.balance - 15;
      this.history.push({ kind: "fee", amount: 15 });
    }
  }
}

class Bank {
  constructor() {
    this.accounts = new Map();
    this.failures = 0;
    this.log = [];
  }
  open(acct) { this.accounts.set(acct.id, acct); return acct; }
  transfer(fromId, toId, amount) {
    const from = this.accounts.get(fromId);
    const to = this.accounts.get(toId);
    try {
      from.withdraw(amount);
      try {
        to.deposit(amount);
      } catch (e) {
        from.deposit(amount);
        throw e;
      }
      return true;
    } catch (e) {
      this.failures++;
      if (!(e instanceof InsufficientFunds) && !(e instanceof RangeError)) throw e;
      return false;
    } finally {
      this.log.push([fromId, toId, amount]);
    }
  }
  monthEnd() { for (const a of this.accounts.values()) a.monthEnd(); }
  total() { let t = 0; for (const a of this.accounts.values()) t += a.balance; return t; }
}

let seed = 7;
function rnd(n) { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed % n; }

const bank = new Bank();
const owners = ["ann", "bob", "cid", "dee", "eve", "fay", "gus", "hal"];
owners.forEach((o, i) => {
  bank.open(i % 2 ? new Savings(o, 500 + i * 50, 0.01) : new Checking(o, 200, 100));
});
const months = Number(process.argv[2] || 12);
let ok = 0;
for (let m = 0; m < months; m++) {
  for (let t = 0; t < 40; t++) {
    const a = 1 + rnd(owners.length), b = 1 + rnd(owners.length);
    const amt = rnd(5) === 0 ? -5 : rnd(300);
    if (a !== b && bank.transfer(a, b, amt)) ok++;
  }
  bank.monthEnd();
}
console.log("ok", ok, "failed", bank.failures, "total", bank.total().toFixed(2));
for (const a of bank.accounts.values()) console.log(String(a));
