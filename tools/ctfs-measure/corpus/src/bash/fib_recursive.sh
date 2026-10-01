#!/usr/bin/env bash
# Recursion-heavy: naive Fibonacci and Ackermann via global return slot.
fib() {
    local n=$1
    if (( n < 2 )); then
        RET=$n
        return
    fi
    local a b
    fib $((n - 1)); a=$RET
    fib $((n - 2)); b=$RET
    RET=$((a + b))
}
ack() {
    local m=$1 n=$2
    if (( m == 0 )); then
        RET=$((n + 1))
    elif (( n == 0 )); then
        ack $((m - 1)) 1
    else
        ack $m $((n - 1))
        ack $((m - 1)) $RET
    fi
}
for k in 5 8 11; do
    fib $k
    echo "fib($k)=$RET"
done
ack 2 2
echo "ack(2,2)=$RET"
