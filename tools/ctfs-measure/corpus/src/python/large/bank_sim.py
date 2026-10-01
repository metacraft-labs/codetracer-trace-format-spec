"""Event-driven OOP simulation: classes, inheritance, properties, generators, exceptions, dataclasses."""
import random
from dataclasses import dataclass, field


class InsufficientFunds(Exception):
    pass


@dataclass
class Txn:
    kind: str
    amount: int
    ok: bool = True


@dataclass
class Account:
    owner: str
    balance: int = 0
    history: list = field(default_factory=list)

    def deposit(self, amount):
        self.balance += amount
        self.history.append(Txn("dep", amount))

    def withdraw(self, amount):
        if amount > self.balance:
            self.history.append(Txn("wd", amount, False))
            raise InsufficientFunds(f"{self.owner}: {amount} > {self.balance}")
        self.balance -= amount
        self.history.append(Txn("wd", amount))

    @property
    def failed(self):
        return sum(1 for t in self.history if not t.ok)


class SavingsAccount(Account):
    rate = 0.01

    def accrue(self):
        interest = int(self.balance * self.rate)
        if interest:
            self.deposit(interest)


def events(rng, accounts, n):
    for i in range(n):
        acct = rng.choice(accounts)
        r = rng.random()
        if r < 0.45:
            yield ("dep", acct, rng.randrange(1, 200))
        elif r < 0.9:
            yield ("wd", acct, rng.randrange(1, 300))
        else:
            yield ("xfer", acct, (rng.choice(accounts), rng.randrange(1, 100)))
        if i % 100 == 99:
            yield ("tick", None, None)


def main():
    rng = random.Random(99)
    accounts = [SavingsAccount(f"s{i}") if i % 2 else Account(f"a{i}") for i in range(12)]
    errors = 0
    for kind, acct, arg in events(rng, accounts, 2500):
        try:
            if kind == "dep":
                acct.deposit(arg)
            elif kind == "wd":
                acct.withdraw(arg)
            elif kind == "xfer":
                dst, amt = arg
                acct.withdraw(amt)
                dst.deposit(amt)
            else:
                for a in accounts:
                    if isinstance(a, SavingsAccount):
                        a.accrue()
        except InsufficientFunds:
            errors += 1
    print("errors", errors, [(a.owner, a.balance, a.failed) for a in accounts[:4]])


main()
