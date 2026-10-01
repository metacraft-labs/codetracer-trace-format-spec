<?php
// Larger realistic workload for trace-format measurement.
class Account {
    public function __construct(public string $owner, private int $balance = 0) {}
    public function deposit(int $amt): void { if ($amt <= 0) throw new InvalidArgumentException("bad"); $this->balance += $amt; }
    public function withdraw(int $amt): bool { if ($amt > $this->balance) return false; $this->balance -= $amt; return true; }
    public function balance(): int { return $this->balance; }
}
function fib(int $n): int { return $n < 2 ? $n : fib($n - 1) + fib($n - 2); }
function merge_sort(array $a): array {
    if (count($a) <= 1) return $a;
    $mid = intdiv(count($a), 2);
    $l = merge_sort(array_slice($a, 0, $mid));
    $r = merge_sort(array_slice($a, $mid));
    $out = []; $i = 0; $j = 0;
    while ($i < count($l) && $j < count($r)) { $out[] = $l[$i] <= $r[$j] ? $l[$i++] : $r[$j++]; }
    while ($i < count($l)) $out[] = $l[$i++];
    while ($j < count($r)) $out[] = $r[$j++];
    return $out;
}
function word_freq(string $text): array {
    $f = [];
    foreach (preg_split('/\s+/', strtolower($text)) as $w) { if ($w === '') continue; $f[$w] = ($f[$w] ?? 0) + 1; }
    arsort($f);
    return $f;
}
$seed = 12345; $data = [];
for ($k = 0; $k < 300; $k++) { $seed = ($seed * 1103515245 + 12345) % 2147483648; $data[] = $seed % 1000; }
$sorted = merge_sort($data);
echo "head: ", implode(',', array_slice($sorted, 0, 5)), "\n";
echo "fib(15)=", fib(15), "\n";
$accts = [];
foreach (['alice', 'bob', 'carol'] as $i => $name) { $a = new Account($name); $a->deposit(100 * ($i + 1)); $accts[] = $a; }
for ($t = 0; $t < 50; $t++) {
    $from = $accts[$t % 3]; $to = $accts[($t + 1) % 3];
    if ($from->withdraw(7 + $t)) { $to->deposit(7 + $t); }
    try { if ($t % 17 == 0) $to->deposit(0); } catch (InvalidArgumentException $e) { echo "caught at $t\n"; }
}
foreach ($accts as $a) echo $a->owner, "=", $a->balance(), "\n";
print_r(array_slice(word_freq("the quick brown fox jumps over the lazy dog the fox barks and the dog runs"), 0, 3));
