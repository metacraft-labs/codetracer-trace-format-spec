// Larger deterministic workload for the solana corpus: sort, prime sieve,
// a toy hash and a small account-ledger simulation.
struct Ledger {
    balances: [u64; 16],
    ops: u64,
}

impl Ledger {
    fn transfer(&mut self, from: usize, to: usize, amount: u64) -> bool {
        if self.balances[from] < amount {
            return false;
        }
        self.balances[from] -= amount;
        self.balances[to] += amount;
        self.ops += 1;
        true
    }
}

fn lcg(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

fn insertion_sort(v: &mut [u64]) {
    let mut i = 1;
    while i < v.len() {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
        i += 1;
    }
}

fn sieve(limit: usize) -> u64 {
    let mut is_prime = [true; 200];
    let mut count = 0u64;
    let mut n = 2;
    while n < limit {
        if is_prime[n] {
            count += 1;
            let mut m = n * n;
            while m < limit {
                is_prime[m] = false;
                m += n;
            }
        }
        n += 1;
    }
    count
}

fn fnv(data: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for x in data {
        h ^= *x;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn compute() -> u64 {
    let mut v = [0u64; 48];
    let mut seed = 42u64;
    for i in 0..v.len() {
        seed = lcg(seed);
        v[i] = seed % 1000;
    }
    insertion_sort(&mut v);
    let primes = sieve(200);
    let mut ledger = Ledger { balances: [100; 16], ops: 0 };
    for k in 0..64u64 {
        seed = lcg(seed);
        let from = (seed % 16) as usize;
        let to = ((seed >> 8) % 16) as usize;
        let ok = ledger.transfer(from, to, (seed >> 16) % 60);
        if !ok {
            ledger.balances[from] += k;
        }
    }
    fnv(&v) ^ primes ^ fnv(&ledger.balances) ^ ledger.ops
}
