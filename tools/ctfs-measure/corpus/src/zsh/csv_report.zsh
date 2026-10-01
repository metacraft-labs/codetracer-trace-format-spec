#!/usr/bin/env zsh
# Record processing: parse CSV lines, validate fields, aggregate by key,
# nested conditionals and early-continue error handling.
declare -A sum count
errors=0
parse_line() {
    local IFS=,
    read -r F_DATE F_REGION F_ITEM F_QTY F_PRICE <<< "$1"
}
validate() {
    [[ $F_QTY =~ ^[0-9]+$ ]] || return 1
    [[ $F_PRICE =~ ^[0-9]+(\.[0-9]{1,2})?$ ]] || return 2
    [[ -n $F_REGION ]] || return 3
    return 0
}
cents() {
    local p=$1
    if [[ $p == *.* ]]; then
        local whole=${p%%.*} frac=${p#*.}
        (( ${#frac} == 1 )) && frac="${frac}0"
        RET=$(( 10#$whole * 100 + 10#$frac ))
    else
        RET=$(( p * 100 ))
    fi
}
while IFS= read -r row; do
    [[ $row == \#* || -z $row ]] && continue
    parse_line "$row"
    if ! validate; then
        errors=$((errors + 1))
        echo "bad row ($?): $row" >&2
        continue
    fi
    cents "$F_PRICE"
    amount=$(( RET * F_QTY ))
    sum[$F_REGION]=$(( ${sum[$F_REGION]:-0} + amount ))
    count[$F_REGION]=$(( ${count[$F_REGION]:-0} + 1 ))
done <<'CSV'
# date,region,item,qty,price
2026-01-02,north,widget,3,4.50
2026-01-02,south,gadget,1,19.99
2026-01-03,north,gizmo,10,0.75
2026-01-03,east,widget,2,4.5
2026-01-04,west,gadget,x,19.99
2026-01-04,south,widget,7,4.50
2026-01-05,,gizmo,1,0.75
2026-01-05,east,gizmo,4,0.80
2026-01-06,north,gadget,2,18
2026-01-06,west,widget,5,4.25
2026-01-07,south,gizmo,12,0.70
2026-01-07,east,gadget,1,abc
2026-01-08,west,gizmo,6,0.75
2026-01-08,north,widget,1,4.50
CSV
for r in north south east west; do
    s=${sum[$r]:-0}
    printf '%-6s n=%d total=%d.%02d\n' "$r" "${count[$r]:-0}" $((s / 100)) $((s % 100))
done
echo "errors: $errors"
