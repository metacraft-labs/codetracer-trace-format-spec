program loops_large.aleo {
    function mix(a: u32, b: u32) -> u32 {
        let x: u32 = a * 31u32;
        let y: u32 = x + b;
        let z: u32 = y % 65521u32;
        return z;
    }

    transition main() -> u32 {
        let mut acc: u32 = 1u32;
        let mut fib_a: u32 = 0u32;
        let mut fib_b: u32 = 1u32;
        for i: u32 in 0u32..40u32 {
            acc = mix(acc, i);
            let t: u32 = (fib_a + fib_b) % 65521u32;
            fib_a = fib_b;
            fib_b = t;
        }
        let result: u32 = acc + fib_b;
        return result;
    }
}
