# Leans on the standard library: Set, StringScanner, Struct, Comparable,
# Enumerable chains, Date/Time, TSort, OptionParser, URI, Forwardable, Digest, Shellwords.
require 'set'
require 'strscan'
require 'tsort'
require 'optparse'
require 'uri'
require 'forwardable'
require 'time'
require 'date'
require 'digest'
require 'shellwords'

Employee = Struct.new(:name, :dept, :salary, :hired) do
  include Comparable
  def <=>(other) = [dept, -salary] <=> [other.dept, -other.salary]
  def tenure(today) = ((today - hired) / 365.25).floor
end

rng = Random.new(7)
depts = %w[eng ops sales legal research]
names = %w[ada brian chen dora eli fay gus hana ivan jo kai lin]
start = Date.new(2010, 1, 1)
staff = Array.new(600) do |i|
  Employee.new("#{names[i % names.size]}#{i}", depts.sample(random: rng),
               rng.rand(40_000..180_000), start + rng.rand(5000))
end

csv_text = (["name,dept,salary,hired"] +
            staff.map { |e| [e.name, e.dept, e.salary, e.hired.iso8601].join(',') }).join("\n")
header, *lines = csv_text.lines(chomp: true)
keys = header.split(',')
rows = lines.map do |line|
  r = keys.zip(line.split(',')).to_h
  Employee.new(r['name'], r['dept'], r['salary'].to_i, Date.iso8601(r['hired']))
end

today = Date.new(2026, 10, 1)
by_dept = rows.group_by(&:dept).transform_values do |es|
  { count: es.size, avg: es.sum(&:salary) / es.size, max_tenure: es.map { |e| e.tenure(today) }.max }
end
seen = Set.new
dups = rows.map { |e| e.name.gsub(/\d+/, '') }.select { |n| !seen.add?(n) }.uniq

tokens = []
ss = StringScanner.new('let x = 42 + foo(3, "bar") * 7; if x > 10 then print x end' * 20)
until ss.eos?
  if ss.skip(/\s+/) then next
  elsif (t = ss.scan(/\d+/)) then tokens << [:int, t.to_i]
  elsif (t = ss.scan(/"[^"]*"/)) then tokens << [:str, t[1..-2]]
  elsif (t = ss.scan(/[A-Za-z_]\w*/)) then tokens << [:id, t]
  else tokens << [:op, ss.getch]
  end
end

report = by_dept.sort.map do |d, s|
  format('%-10s %3d %8d %2d', d, s[:count], s[:avg], s[:max_tenure])
end.join("\n")

class Deps
  include TSort
  extend Forwardable
  def_delegators :@h, :[], :size
  def initialize(h) = @h = h
  def tsort_each_node(&b) = @h.each_key(&b)
  def tsort_each_child(n, &b) = @h.fetch(n, []).each(&b)
end
deps = Deps.new((0...40).to_h { |i| ["m#{i}", (0...i).select { |j| (i * 7 + j) % 5 == 0 }.map { |j| "m#{j}" }] })
puts deps.tsort.last(3).inspect, deps.strongly_connected_components.size

opts = {}
parser = OptionParser.new do |o|
  o.on('-v', '--verbose') { opts[:verbose] = true }
  o.on('-nNAME', '--name NAME', String) { |v| opts[:name] = v }
  o.on('--count N', Integer) { |v| opts[:count] = v }
  o.on('--tags x,y', Array) { |v| opts[:tags] = v }
end
rest = parser.parse(%w[-v --name trace --count 12 --tags a,b,c file1 file2])
puts opts.inspect, rest.inspect

urls = Array.new(200) { |i| "https://host#{i % 4}.example.com:#{8000 + i}/p/#{i}?q=#{i * 3}&r=x#frag#{i}" }
puts urls.map { |u| URI.parse(u) }.group_by(&:host).transform_values { |v| v.sum(&:port) }.inspect
puts Time.iso8601('2026-10-01T12:34:56Z').strftime('%Y/%j %H:%M')
puts report
puts Digest::SHA256.hexdigest(report)[0, 16]
puts rows.sort.first(3).map(&:name).join(',')
puts dups.inspect
puts tokens.map(&:first).tally.inspect
puts Shellwords.split(%q(cmd --opt "a b" 'c d' e\ f)).inspect
puts rows.each_slice(10).map { |s| s.minmax_by(&:salary).map(&:salary) }.flatten.sum
