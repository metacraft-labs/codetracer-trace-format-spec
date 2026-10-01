#![allow(dead_code, unused_imports)]
// Larger realistic workload for trace-format measurement: tokenizer +
// expression evaluator, insertion/merge sort, word frequency, matrix mult.
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum Tok { Num(i64), Op(char), LParen, RParen }

fn tokenize(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let mut v = 0i64;
            while i < chars.len() && chars[i].is_ascii_digit() {
                v = v * 10 + chars[i].to_digit(10).unwrap() as i64;
                i += 1;
            }
            out.push(Tok::Num(v));
            continue;
        }
        match c {
            '+' | '-' | '*' | '/' => out.push(Tok::Op(c)),
            '(' => out.push(Tok::LParen),
            ')' => out.push(Tok::RParen),
            _ => {}
        }
        i += 1;
    }
    out
}

struct Parser { toks: Vec<Tok>, pos: usize }

impl Parser {
    fn peek(&self) -> Option<&Tok> { self.toks.get(self.pos) }
    fn expr(&mut self) -> i64 {
        let mut acc = self.term();
        while let Some(Tok::Op(op)) = self.peek().cloned() {
            if op != '+' && op != '-' { break; }
            self.pos += 1;
            let rhs = self.term();
            acc = if op == '+' { acc + rhs } else { acc - rhs };
        }
        acc
    }
    fn term(&mut self) -> i64 {
        let mut acc = self.atom();
        while let Some(Tok::Op(op)) = self.peek().cloned() {
            if op != '*' && op != '/' { break; }
            self.pos += 1;
            let rhs = self.atom();
            acc = if op == '*' { acc * rhs } else if rhs != 0 { acc / rhs } else { 0 };
        }
        acc
    }
    fn atom(&mut self) -> i64 {
        match self.peek().cloned() {
            Some(Tok::Num(v)) => { self.pos += 1; v }
            Some(Tok::LParen) => { self.pos += 1; let v = self.expr(); self.pos += 1; v }
            _ => { self.pos += 1; 0 }
        }
    }
}

fn merge_sort(v: &mut Vec<i32>) {
    if v.len() <= 1 { return; }
    let mid = v.len() / 2;
    let mut left = v[..mid].to_vec();
    let mut right = v[mid..].to_vec();
    merge_sort(&mut left);
    merge_sort(&mut right);
    let (mut i, mut j, mut k) = (0, 0, 0);
    while i < left.len() && j < right.len() {
        if left[i] <= right[j] { v[k] = left[i]; i += 1; } else { v[k] = right[j]; j += 1; }
        k += 1;
    }
    while i < left.len() { v[k] = left[i]; i += 1; k += 1; }
    while j < right.len() { v[k] = right[j]; j += 1; k += 1; }
}

fn matmul(a: &[[f64; 6]; 6], b: &[[f64; 6]; 6]) -> [[f64; 6]; 6] {
    let mut c = [[0.0; 6]; 6];
    for i in 0..6 { for j in 0..6 { let mut s = 0.0; for k in 0..6 { s += a[i][k] * b[k][j]; } c[i][j] = s; } }
    c
}

fn main() {
    let exprs = ["1+2*3", "(4+5)*(6-2)", "100/7+3*(2+8)", "((1+2)*(3+4))*(5+6)-7", "42"];
    let mut total = 0;
    for e in exprs.iter() { let mut p = Parser { toks: tokenize(e), pos: 0 }; let v = p.expr(); total += v; println!("{} = {}", e, v); }
    println!("{}", total);
}
