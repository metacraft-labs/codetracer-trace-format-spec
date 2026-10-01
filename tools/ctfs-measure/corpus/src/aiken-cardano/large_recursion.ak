fn sum_acc(n: Int, acc: Int) -> Int {
  when n is {
    0 -> acc
    _ -> sum_acc(n - 1, acc + n)
  }
}

fn fib(n: Int) -> Int {
  when n is {
    0 -> 0
    1 -> 1
    _ -> fib(n - 1) + fib(n - 2)
  }
}

fn pow_acc(base: Int, exp: Int, acc: Int) -> Int {
  when exp is {
    0 -> acc
    _ -> pow_acc(base, exp - 1, acc * base)
  }
}

fn compute() -> Int {
  let s = sum_acc(150, 0)
  let f = fib(12)
  let c = pow_acc(3, 20, 1) % 1000
  let total = s + f + c
  total
}

test large_recursion() {
  compute() == 11325 + 144 + 401
}
