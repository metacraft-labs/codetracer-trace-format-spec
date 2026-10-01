# A tiny Lisp: tokenizer, reader, environment chain, evaluator with closures and tail-ish loops.
class Env
  def initialize(vars = {}, outer = nil)
    @vars = vars
    @outer = outer
  end

  def find(name)
    return self if @vars.key?(name)
    raise NameError, "unbound #{name}" if @outer.nil?
    @outer.find(name)
  end

  def [](name) = find(name).vars[name]
  def []=(name, v)
    @vars[name] = v
  end
  def set!(name, v)
    find(name).vars[name] = v
  end
  protected attr_reader :vars
end

Lambda = Struct.new(:params, :body, :env)

def tokenize(src) = src.gsub('(', ' ( ').gsub(')', ' ) ').split

def read(tokens)
  tok = tokens.shift
  case tok
  when '('
    list = []
    list << read(tokens) while tokens.first != ')'
    tokens.shift
    list
  when /\A-?\d+\z/ then tok.to_i
  else tok.to_sym
  end
end

def global_env
  Env.new({
    :+ => ->(*a) { a.sum }, :- => ->(a, b) { a - b }, :* => ->(a, b) { a * b },
    :< => ->(a, b) { a < b }, :'=' => ->(a, b) { a == b }, :mod => ->(a, b) { a % b },
    :list => ->(*a) { a }, :car => ->(l) { l.first }, :cdr => ->(l) { l.drop(1) },
    :cons => ->(x, l) { [x] + l }, :'null?' => ->(l) { l.empty? }
  })
end

def evaluate(x, env)
  case x
  when Symbol then env[x]
  when Integer then x
  when Array
    op, *args = x
    case op
    when :quote then args[0]
    when :if
      evaluate(args[0], env) ? evaluate(args[1], env) : evaluate(args[2], env)
    when :define then env[args[0]] = evaluate(args[1], env)
    when :set! then env.set!(args[0], evaluate(args[1], env))
    when :lambda then Lambda.new(args[0], args[1], env)
    when :begin
      result = nil
      args.each { |e| result = evaluate(e, env) }
      result
    when :while
      evaluate(args[1], env) while evaluate(args[0], env)
      nil
    else
      f = evaluate(op, env)
      vals = args.map { |a| evaluate(a, env) }
      apply(f, vals)
    end
  end
end

def apply(f, vals)
  if f.is_a?(Lambda)
    evaluate(f.body, Env.new(f.params.zip(vals).to_h, f.env))
  else
    f.call(*vals)
  end
end

PROGRAM = <<~LISP
  (begin
    (define fact (lambda (n) (if (< n 2) 1 (* n (fact (- n 1))))))
    (define fib (lambda (n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))))
    (define map (lambda (f l) (if (null? l) (quote ()) (cons (f (car l)) (map f (cdr l))))))
    (define range (lambda (a b) (if (< a b) (cons a (range (+ a 1) b)) (quote ()))))
    (define sum 0)
    (define i 0)
    (while (< i 200) (begin (if (= (mod i 3) 0) (set! sum (+ sum i)) 0) (set! i (+ i 1))))
    (list (fact 12) (fib 12) sum (map (lambda (x) (* x x)) (range 0 15))))
LISP

env = global_env
p evaluate(read(tokenize(PROGRAM)), env)
begin
  evaluate(read(tokenize('(undefined-fn 1)')), env)
rescue NameError => e
  puts "error: #{e.message}"
end
