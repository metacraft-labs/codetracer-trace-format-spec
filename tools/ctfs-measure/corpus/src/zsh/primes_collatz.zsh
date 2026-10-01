#!/usr/bin/env zsh
# Numeric loops: trial-division primes, gcd, and Collatz sequence lengths.
is_prime() {
    local n=$1 d
    (( n < 2 )) && return 1
    for (( d = 2; d * d <= n; d++ )); do
        (( n % d == 0 )) && return 1
    done
    return 0
}
gcd() {
    local a=$1 b=$2 t
    while (( b != 0 )); do
        t=$((a % b)); a=$b; b=$t
    done
    RET=$a
}
collatz_len() {
    local n=$1 steps=0
    while (( n != 1 )); do
        if (( n % 2 == 0 )); then n=$((n / 2)); else n=$((3 * n + 1)); fi
        steps=$((steps + 1))
    done
    RET=$steps
}
count=0
for (( k = 2; k < 150; k++ )); do
    if is_prime $k; then count=$((count + 1)); fi
done
echo "primes below 150: $count"
gcd 1071 462; echo "gcd=$RET"
gcd 832040 514229; echo "gcd=$RET"
best=0; best_n=0
for (( k = 1; k < 30; k++ )); do
    collatz_len $k
    if (( RET > best )); then best=$RET; best_n=$k; fi
done
echo "longest collatz below 30: $best_n ($best steps)"
