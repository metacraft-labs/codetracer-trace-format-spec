#!/usr/bin/env zsh
# Interpreter: a small Brainfuck VM (case dispatch, bracket matching).
# Native 1-based zsh arrays; the program counter is 0-based into the string.
prog='++++++++[>++++[>++>+++>+++>+<<<<-]>+>+>->>+[<]<-]>>.>---.+++++++..+++.>>.<-.<.+++.------.--------.>>+.'
typeset -a tape=(0 0 0 0 0 0 0 0)
typeset -A jump
# Precompute bracket matches with an explicit stack.
stack=()
for (( i = 0; i < ${#prog}; i++ )); do
    c=${prog:$i:1}
    if [[ $c == "[" ]]; then
        stack+=("$i")
    elif [[ $c == "]" ]]; then
        open=${stack[-1]}
        stack[-1]=()
        jump[$open]=$i
        jump[$i]=$open
    fi
done
pc=0; dp=1; out=""
plen=${#prog}
while (( pc < plen )); do
    c=${prog:$pc:1}
    case $c in
        "+") tape[dp]=$(( (tape[dp] + 1) % 256 )) ;;
        "-") tape[dp]=$(( (tape[dp] + 255) % 256 )) ;;
        ">") dp=$((dp + 1)) ;;
        "<") dp=$((dp - 1)) ;;
        ".") out+=${(#)tape[dp]} ;;
        "[") (( tape[dp] == 0 )) && pc=${jump[$pc]} ;;
        "]") (( tape[dp] != 0 )) && pc=${jump[$pc]} ;;
    esac
    pc=$((pc + 1))
done
echo "$out"
