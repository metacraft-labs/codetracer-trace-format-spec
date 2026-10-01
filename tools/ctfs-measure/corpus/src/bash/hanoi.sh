#!/usr/bin/env bash
# Recursion with output: Towers of Hanoi plus permutation generation.
moves=0
hanoi() {
    local n=$1 from=$2 to=$3 via=$4
    if (( n == 0 )); then
        return
    fi
    hanoi $((n - 1)) "$from" "$via" "$to"
    moves=$((moves + 1))
    (( moves % 16 == 0 )) && echo "move $moves: disk $n $from -> $to"
    hanoi $((n - 1)) "$via" "$to" "$from"
}
perms=0
permute() {
    local prefix=$1 rest=$2 i
    if [[ -z "$rest" ]]; then
        perms=$((perms + 1))
        return
    fi
    for (( i = 0; i < ${#rest}; i++ )); do
        permute "$prefix${rest:i:1}" "${rest:0:i}${rest:i+1}"
    done
}
hanoi 7 A C B
echo "total moves: $moves"
permute "" "abcd"
echo "permutations: $perms"
