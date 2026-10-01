fn lcg(seed: u64) -> u64 {
    (seed * 1103515245 + 12345) % 2147483648
}

fn fib(n: u32) -> u128 {
    let mut a: u128 = 0;
    let mut b: u128 = 1;
    let mut i: u32 = 0;
    while i < n {
        let t = a + b;
        a = b;
        b = t;
        i = i + 1;
    };
    a
}

fn bubble_sort(mut arr: Array<u64>) -> Array<u64> {
    let n = arr.len();
    let mut pass: u32 = 0;
    while pass < n {
        let mut out: Array<u64> = ArrayTrait::new();
        let mut cur: u64 = *arr.at(0);
        let mut j: u32 = 1;
        while j < n {
            let next = *arr.at(j);
            if cur > next {
                out.append(next);
            } else {
                out.append(cur);
                cur = next;
            }
            j = j + 1;
        };
        out.append(cur);
        arr = out;
        pass = pass + 1;
    };
    arr
}

fn checksum(arr: @Array<u64>) -> u64 {
    let mut acc: u64 = 0;
    let mut i: u32 = 0;
    while i < arr.len() {
        acc = (acc * 31 + *arr.at(i)) % 1000000007;
        i = i + 1;
    };
    acc
}

fn main() -> u64 {
    let mut arr: Array<u64> = ArrayTrait::new();
    let mut seed: u64 = 7;
    let mut i: u32 = 0;
    while i < 30 {
        seed = lcg(seed);
        arr.append(seed % 1000);
        i = i + 1;
    };
    let sorted = bubble_sort(arr);
    let f: u128 = fib(60);
    let c = checksum(@sorted);
    c + (f % 1000).try_into().unwrap()
}
