script;

struct Account { id: u64, balance: u64 }

fn lcg(seed: u64) -> u64 { (seed * 1103515245 + 12345) % 2147483648 }

fn gcd(a: u64, b: u64) -> u64 {
    let mut x = a;
    let mut y = b;
    while y != 0 {
        let r = x % y;
        x = y;
        y = r;
    }
    x
}

fn sieve(n: u64) -> u64 {
    let mut flags: Vec<bool> = Vec::new();
    let mut i = 0;
    while i <= n { flags.push(true); i += 1; }
    let mut count = 0;
    let mut p = 2;
    while p <= n {
        if flags.get(p).unwrap() {
            count += 1;
            let mut m = p * p;
            while m <= n { flags.set(m, false); m += p; }
        }
        p += 1;
    }
    count
}

fn insertion_sort(ref mut v: Vec<u64>) {
    let n = v.len();
    let mut i = 1;
    while i < n {
        let mut j = i;
        while j > 0 && v.get(j - 1).unwrap() > v.get(j).unwrap() {
            v.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
}

fn main() -> u64 {
    let mut v: Vec<u64> = Vec::new();
    let mut s = 42;
    let mut i = 0;
    while i < 50 { s = lcg(s); v.push(s % 1000); i += 1; }
    insertion_sort(v);
    let primes = sieve(300);
    let mut g = 0;
    let mut k = 1;
    while k < 30 { g += gcd(k * 7919, 104729 % (k + 3) + 1); k += 1; }
    let mut accts: Vec<Account> = Vec::new();
    let mut a = 0;
    while a < 8 { accts.push(Account { id: a, balance: 100 }); a += 1; }
    let mut t = 0;
    let mut ok = 0;
    while t < 40 {
        s = lcg(s);
        let from = s % 8;
        let to = (s / 8) % 8;
        let amt = s % 60;
        let fa = accts.get(from).unwrap();
        if fa.balance >= amt {
            accts.set(from, Account { id: fa.id, balance: fa.balance - amt });
            let ta = accts.get(to).unwrap();
            accts.set(to, Account { id: ta.id, balance: ta.balance + amt });
            ok += 1;
        }
        t += 1;
    }
    primes + g + ok + v.get(0).unwrap()
}
