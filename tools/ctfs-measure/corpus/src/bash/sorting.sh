#!/usr/bin/env bash
# Loop-heavy: insertion sort, bubble sort and binary search over small arrays.
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
    local i j key n=${#data[@]}
    for (( i = 1; i < n; i++ )); do
        key=${data[i]}
        j=$((i - 1))
        while (( j >= 0 && data[j] > key )); do
            data[j+1]=${data[j]}
            j=$((j - 1))
        done
        data[j+1]=$key
    done
}
bubble_sort() {
    local i j tmp swapped n=${#data[@]}
    for (( i = 0; i < n - 1; i++ )); do
        swapped=0
        for (( j = 0; j < n - i - 1; j++ )); do
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
    local target=$1 lo=0 hi=$(( ${#data[@]} - 1 )) mid
    while (( lo <= hi )); do
        mid=$(( (lo + hi) / 2 ))
        if (( data[mid] == target )); then RET=$mid; return 0
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
for t in "${data[3]}" 500 "${data[17]}" 7; do
    if bsearch "$t"; then echo "found $t at $RET"; else echo "missing $t"; fi
done
