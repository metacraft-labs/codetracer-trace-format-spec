fun fib(n: int): int {
    if (n < 2) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fun gcd(a: int, b: int): int {
    while (b != 0) {
        var t: int = b;
        b = a % b;
        a = t;
    }
    return a;
}

fun collatz_steps(start: int): int {
    var n: int = start;
    var steps: int = 0;
    while (n != 1) {
        if (n % 2 == 0) {
            n = n / 2;
        } else {
            n = 3 * n + 1;
        }
        steps = steps + 1;
    }
    return steps;
}

fun sum_squares(limit: int): int {
    var total: int = 0;
    var i: int = 0;
    while (i < limit) {
        total = total + i * i;
        i = i + 1;
    }
    return total;
}

fun compute(): int {
    var f: int = fib(13);
    var g: int = gcd(1071, 462);
    var c: int = collatz_steps(97);
    var s: int = sum_squares(300);
    var acc: int = 0;
    repeat (50) {
        acc = acc + gcd(acc + 12, 18);
    }
    return f + g + c + s + acc;
}

fun main(): int {
    return compute();
}
