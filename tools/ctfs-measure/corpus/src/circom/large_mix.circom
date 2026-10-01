pragma circom 2.0.0;

function fib(n) {
    var a = 0;
    var b = 1;
    for (var i = 0; i < n; i++) {
        var t = a + b;
        a = b;
        b = t;
    }
    return a;
}

template Mix(k) {
    signal input x;
    signal output y;
    signal sq;
    sq <== x * x;
    var acc = 0;
    for (var i = 0; i < 8; i++) {
        if (i % 2 == 0) {
            acc = acc + i * k;
        } else {
            acc = acc + 3;
        }
    }
    y <== sq + acc;
}

template Chain(N) {
    signal output out;
    component m[N];
    var total = 0;
    for (var i = 0; i < N; i++) {
        m[i] = Mix(i);
        if (i == 0) {
            m[i].x <== 1;
        } else {
            m[i].x <== m[i-1].y - m[i-1].y + i;
        }
        for (var j = 0; j < 6; j++) {
            total = total + i * j;
        }
    }
    out <== m[N-1].y + total + fib(20);
}

component main = Chain(40);
