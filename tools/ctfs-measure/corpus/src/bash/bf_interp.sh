#!/usr/bin/env bash
# Interpreter: a small Brainfuck VM (case dispatch, bracket matching).
prog='++++++++[>++++[>++>+++>+++>+<<<<-]>+>+>->>+[<]<-]>>.>---.+++++++..+++.>>.<-.<.+++.------.--------.>>+.'
declare -a tape=(0 0 0 0 0 0 0 0)
declare -a jump
# Precompute bracket matches with an explicit stack.
stack=()
for (( i = 0; i < ${#prog}; i++ )); do
    c=${prog:i:1}
    if [[ $c == "[" ]]; then
        stack+=("$i")
    elif [[ $c == "]" ]]; then
        open=${stack[-1]}
        unset 'stack[-1]'
        jump[open]=$i
        jump[i]=$open
    fi
done
pc=0; dp=0; out=""
plen=${#prog}
while (( pc < plen )); do
    c=${prog:pc:1}
    case $c in
        "+") tape[dp]=$(( (tape[dp] + 1) % 256 )) ;;
        "-") tape[dp]=$(( (tape[dp] + 255) % 256 )) ;;
        ">") dp=$((dp + 1)) ;;
        "<") dp=$((dp - 1)) ;;
        ".") printf -v ch "\\$(printf '%03o' "${tape[dp]}")"; out+=$ch ;;
        "[") (( tape[dp] == 0 )) && pc=${jump[pc]} ;;
        "]") (( tape[dp] != 0 )) && pc=${jump[pc]} ;;
    esac
    pc=$((pc + 1))
done
echo "$out"
