# Recursive-descent JSON parser and pretty-printer over a generated document.
class JsonParser
  class ParseError < StandardError; end

  def initialize(src)
    @src = src
    @pos = 0
  end

  def parse
    skip_ws
    v = parse_value
    skip_ws
    raise ParseError, "trailing data at #{@pos}" if @pos < @src.length
    v
  end

  private

  def peek = @src[@pos]

  def skip_ws
    @pos += 1 while @pos < @src.length && " \t\n\r".include?(@src[@pos])
  end

  def expect(ch)
    raise ParseError, "expected #{ch} at #{@pos}" unless @src[@pos] == ch
    @pos += 1
  end

  def parse_value
    case peek
    when '{' then parse_object
    when '[' then parse_array
    when '"' then parse_string
    when 't' then literal('true', true)
    when 'f' then literal('false', false)
    when 'n' then literal('null', nil)
    else parse_number
    end
  end

  def literal(word, value)
    raise ParseError, "bad literal at #{@pos}" unless @src[@pos, word.length] == word
    @pos += word.length
    value
  end

  def parse_object
    expect('{')
    obj = {}
    skip_ws
    if peek == '}'
      @pos += 1
      return obj
    end
    loop do
      skip_ws
      key = parse_string
      skip_ws
      expect(':')
      skip_ws
      obj[key] = parse_value
      skip_ws
      if peek == ','
        @pos += 1
      else
        expect('}')
        break
      end
    end
    obj
  end

  def parse_array
    expect('[')
    arr = []
    skip_ws
    if peek == ']'
      @pos += 1
      return arr
    end
    loop do
      skip_ws
      arr << parse_value
      skip_ws
      if peek == ','
        @pos += 1
      else
        expect(']')
        break
      end
    end
    arr
  end

  def parse_string
    expect('"')
    out = +''
    while (c = @src[@pos]) != '"'
      if c == '\\'
        @pos += 1
        esc = @src[@pos]
        out << case esc
               when 'n' then "\n"
               when 't' then "\t"
               when 'u'
                 code = @src[@pos + 1, 4].to_i(16)
                 @pos += 4
                 code.chr(Encoding::UTF_8)
               else esc
               end
      else
        out << c
      end
      @pos += 1
    end
    @pos += 1
    out
  end

  def parse_number
    start = @pos
    @pos += 1 if peek == '-'
    @pos += 1 while @pos < @src.length && @src[@pos] =~ /[0-9.eE+-]/
    text = @src[start...@pos]
    raise ParseError, "bad number at #{start}" if text.empty?
    text.include?('.') || text.include?('e') ? text.to_f : text.to_i
  end
end

def generate(depth, seed)
  r = Random.new(seed)
  build = lambda do |d|
    kind = d.zero? ? r.rand(4) : r.rand(6)
    case kind
    when 0 then r.rand(-1000..1000)
    when 1 then (r.rand * 100).round(3)
    when 2 then "s#{r.rand(10_000)}\\n\\u0041"
    when 3 then [true, false, nil][r.rand(3)]
    when 4 then Array.new(r.rand(1..4)) { build.(d - 1) }
    else Array.new(r.rand(1..4)) { |i| ["k#{i}", build.(d - 1)] }.to_h
    end
  end
  build.(depth)
end

def dump(v, indent = 0)
  pad = '  ' * indent
  case v
  when Hash
    return '{}' if v.empty?
    inner = v.map { |k, x| "#{pad}  \"#{k}\": #{dump(x, indent + 1)}" }
    "{\n#{inner.join(",\n")}\n#{pad}}"
  when Array
    return '[]' if v.empty?
    "[#{v.map { |x| dump(x, indent + 1) }.join(', ')}]"
  when String then "\"#{v.gsub("\n", '\\n')}\""
  when nil then 'null'
  else v.to_s
  end
end

def count_nodes(v)
  case v
  when Hash then 1 + v.values.sum { |x| count_nodes(x) }
  when Array then 1 + v.sum { |x| count_nodes(x) }
  else 1
  end
end

total = 0
40.times do |i|
  doc = { 'id' => i, 'payload' => generate(6, i) }
  text = dump(doc)
  text = text.gsub('\\\\n', '\\n')
  parsed = JsonParser.new(text).parse
  total += count_nodes(parsed)
end
begin
  JsonParser.new('{"a": [1, 2,, 3]}').parse
rescue JsonParser::ParseError => e
  puts "error: #{e.message}"
end
puts "nodes: #{total}"
