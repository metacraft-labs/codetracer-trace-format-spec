#!/usr/bin/env bash
# Larger workload: Conway's Game of Life on a 10x10 toroidal grid stored in
# a string; nested loops, modular arithmetic, neighbour counting.
W=10
H=10
grid=".........."
grid+="...#......"
grid+="....#....."
grid+="..###....."
grid+=".........."
grid+=".........."
grid+="......##.."
grid+="......##.."
grid+=".........."
grid+=".........."
cell() {
    local x=$(( ($1 + W) % W )) y=$(( ($2 + H) % H ))
    [[ ${grid:$((y * W + x)):1} == "#" ]]
}
neighbours() {
    local x=$1 y=$2 dx dy n=0
    for dy in -1 0 1; do
        for dx in -1 0 1; do
            (( dx == 0 && dy == 0 )) && continue
            cell $((x + dx)) $((y + dy)) && n=$((n + 1))
        done
    done
    RET=$n
}
step() {
    local x y next="" alive
    for (( y = 0; y < H; y++ )); do
        for (( x = 0; x < W; x++ )); do
            neighbours $x $y
            if cell $x $y; then alive=1; else alive=0; fi
            if (( RET == 3 || (alive == 1 && RET == 2) )); then
                next+="#"
            else
                next+="."
            fi
        done
    done
    grid=$next
}
show() {
    local y
    for (( y = 0; y < H; y++ )); do
        echo "${grid:$((y * W)):$W}"
    done
    echo
}
for gen in 1 2 3; do
    step
done
show
