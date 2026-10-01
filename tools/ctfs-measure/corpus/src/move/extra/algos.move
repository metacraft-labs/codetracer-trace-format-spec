module flow_test::algos {
    use std::vector;

    public struct Account has drop, copy { id: u64, balance: u64 }

    fun lcg(seed: u64): u64 { (seed * 1103515245 + 12345) % 2147483648 }

    fun make_data(n: u64): vector<u64> {
        let mut v = vector::empty<u64>();
        let mut s = 42;
        let mut i = 0;
        while (i < n) { s = lcg(s); vector::push_back(&mut v, s % 1000); i = i + 1; };
        v
    }

    fun insertion_sort(v: &mut vector<u64>) {
        let n = vector::length(v);
        let mut i = 1;
        while (i < n) {
            let mut j = i;
            while (j > 0 && *vector::borrow(v, j - 1) > *vector::borrow(v, j)) {
                vector::swap(v, j - 1, j);
                j = j - 1;
            };
            i = i + 1;
        }
    }

    fun sieve(n: u64): u64 {
        let mut flags = vector::empty<bool>();
        let mut i = 0;
        while (i <= n) { vector::push_back(&mut flags, true); i = i + 1; };
        let mut count = 0;
        let mut p = 2;
        while (p <= n) {
            if (*vector::borrow(&flags, p)) {
                count = count + 1;
                let mut m = p * p;
                while (m <= n) { *vector::borrow_mut(&mut flags, m) = false; m = m + p; };
            };
            p = p + 1;
        };
        count
    }

    fun gcd(a: u64, b: u64): u64 { if (b == 0) a else gcd(b, a % b) }

    fun transfer(accts: &mut vector<Account>, from: u64, to: u64, amt: u64): bool {
        let fb = vector::borrow(accts, from).balance;
        if (fb < amt) return false;
        vector::borrow_mut(accts, from).balance = fb - amt;
        let tb = vector::borrow(accts, to).balance;
        vector::borrow_mut(accts, to).balance = tb + amt;
        true
    }

    #[test]
    fun test_algos() {
        let mut v = make_data(60);
        insertion_sort(&mut v);
        let primes = sieve(400);
        assert!(primes == 78, 1);
        let mut g = 0;
        let mut i = 1;
        while (i < 40) { g = g + gcd(i * 7919, 104729 % (i + 3) + 1); i = i + 1; };
        let mut accts = vector::empty<Account>();
        let mut k = 0;
        while (k < 8) { vector::push_back(&mut accts, Account { id: k, balance: 100 }); k = k + 1; };
        let mut s = 7;
        let mut t = 0;
        let mut ok = 0;
        while (t < 50) {
            s = lcg(s);
            if (transfer(&mut accts, s % 8, (s / 8) % 8, s % 60)) ok = ok + 1;
            t = t + 1;
        };
        assert!(ok > 0 && g > 0, 2);
    }
}
