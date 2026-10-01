#!/usr/bin/env zsh
# Loop-heavy: insertion sort, bubble sort and binary search over small arrays
# (native 1-based zsh arrays).
seed=12345
rand() {
    seed=$(( (seed * 1103515245 + 12345) % 2147483648 ))
    RET=$(( seed % 1000 ))
}
make_data() {
    data=()
    local i
    for (( i = 0; i < $1; i++ )); do
        rand
        data+=("$RET")
    done
}
insertion_sort() {
    local i j key n=${#data}
    for (( i = 2; i <= n; i++ )); do
        key=${data[i]}
        j=$((i - 1))
        while (( j >= 1 && data[j] > key )); do
            data[j+1]=${data[j]}
            j=$((j - 1))
        done
        data[j+1]=$key
    done
}
bubble_sort() {
    local i j tmp swapped n=${#data}
    for (( i = 1; i < n; i++ )); do
        swapped=0
        for (( j = 1; j <= n - i; j++ )); do
            if (( data[j] > data[j+1] )); then
                tmp=${data[j]}
                data[j]=${data[j+1]}
                data[j+1]=$tmp
                swapped=1
            fi
        done
        (( swapped == 0 )) && break
    done
}
bsearch() {
    local target=$1 lo=1 hi=${#data} mid
    while (( lo <= hi )); do
        mid=$(( (lo + hi) / 2 ))
        if (( data[mid] == target )); then RET=$((mid - 1)); return 0
        elif (( data[mid] < target )); then lo=$((mid + 1))
        else hi=$((mid - 1)); fi
    done
    RET=-1
    return 1
}
make_data 24
insertion_sort
echo "insertion: ${data[*]}"
make_data 20
bubble_sort
echo "bubble: ${data[*]}"
for t in "${data[4]}" 500 "${data[18]}" 7; do
    if bsearch "$t"; then echo "found $t at $RET"; else echo "missing $t"; fi
done
