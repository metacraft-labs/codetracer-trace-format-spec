#!/usr/bin/env zsh
# Text processing: word frequency with associative arrays, case folding,
# parameter-expansion string ops, while-read loops over a here-doc.
declare -A freq
total=0
longest=""
while IFS= read -r line; do
    line=${(L)line}
    line=${line//[^a-z ]/ }
    for w in ${=line}; do
        (( ${#w} < 3 )) && continue
        freq[$w]=$(( ${freq[$w]:-0} + 1 ))
        total=$((total + 1))
        if (( ${#w} > ${#longest} )); then longest=$w; fi
    done
done <<'TEXT'
The quick brown fox jumps over the lazy dog. The dog sleeps; the fox runs.
A shell script is a computer program designed to be run by a Unix shell,
a command-line interpreter. The various dialects of shell scripts are
considered to be scripting languages. Typical operations performed by
shell scripts include file manipulation, program execution, and printing text.
A script which sets up the environment, runs the program, and does any
necessary cleanup or logging, is called a wrapper.
TEXT
echo "total words: $total"
echo "distinct: ${#freq[@]}"
echo "longest: $longest"
for w in shell script program the fox; do
    printf '%-8s %d\n' "$w" "${freq[$w]:-0}"
done
