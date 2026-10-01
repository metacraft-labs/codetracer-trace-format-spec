#!/usr/bin/env bash
# Parser: recursive-descent evaluator for integer arithmetic expressions.
src=""
pos=0
peek() {
    while [[ "${src:pos:1}" == " " ]]; do pos=$((pos + 1)); done
    CH=${src:pos:1}
}
parse_number() {
    local num=""
    peek
    while [[ "${src:pos:1}" == [0-9] ]]; do
        num+=${src:pos:1}
        pos=$((pos + 1))
    done
    RET=$num
}
parse_primary() {
    peek
    case "$CH" in
        "(")
            pos=$((pos + 1))
            parse_expr
            peek
            pos=$((pos + 1))
            ;;
        "-")
            pos=$((pos + 1))
            parse_primary
            RET=$(( -RET ))
            ;;
        *)
            parse_number
            ;;
    esac
}
parse_term() {
    local acc op
    parse_primary
    acc=$RET
    while :; do
        peek
        case "$CH" in
            "*"|"/"|"%")
                op=$CH
                pos=$((pos + 1))
                parse_primary
                case $op in
                    "*") acc=$((acc * RET)) ;;
                    "/") acc=$((acc / RET)) ;;
                    "%") acc=$((acc % RET)) ;;
                esac
                ;;
            *) break ;;
        esac
    done
    RET=$acc
}
parse_expr() {
    local acc op
    parse_term
    acc=$RET
    while :; do
        peek
        if [[ "$CH" == "+" || "$CH" == "-" ]]; then
            op=$CH
            pos=$((pos + 1))
            parse_term
            if [[ $op == "+" ]]; then acc=$((acc + RET)); else acc=$((acc - RET)); fi
        else
            break
        fi
    done
    RET=$acc
}
evaluate() {
    src=$1
    pos=0
    parse_expr
    echo "$1 = $RET"
}
evaluate "1 + 2 * 3"
evaluate "(4 + 5) * (2 - 7)"
evaluate "100 / 7 % 4 + -3"
evaluate "((1+2)*(3+4)*(5+6)) - 17 * (2 + (3 * (4 - 1)))"
evaluate "12 * 34 - 56 / 7 + 89 % 10 * (1 + 2 + 3 + 4)"
