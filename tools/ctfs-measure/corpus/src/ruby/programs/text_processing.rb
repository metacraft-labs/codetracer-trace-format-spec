# A mini Markdown-to-HTML converter plus word statistics over generated prose.
WORDS = %w[the quick brown fox jumps over lazy dog trace step record value
           function module return loop branch parser token stream buffer].freeze

def make_doc(rng, sections)
  out = []
  sections.times do |s|
    out << "# Section #{s + 1}"
    out << ''
    rng.rand(2..4).times do
      sentence = Array.new(rng.rand(6..14)) { WORDS.sample(random: rng) }
      sentence[rng.rand(sentence.size)] = "**#{sentence.last}**" if rng.rand < 0.4
      sentence[rng.rand(sentence.size)] = "`code_#{rng.rand(9)}`" if rng.rand < 0.3
      out << sentence.join(' ').capitalize + '.'
    end
    out << ''
    rng.rand(0..4).times { |i| out << "- item #{i} [link](http://x/#{i})" }
    out << ''
    if rng.rand < 0.5
      out << '```'
      out << "def f#{s}(x) = x * #{s}"
      out << '```'
      out << ''
    end
  end
  out.join("\n")
end

def inline(text)
  text = text.gsub('&', '&amp;').gsub('<', '&lt;')
  text = text.gsub(/\*\*(.+?)\*\*/) { "<strong>#{$1}</strong>" }
  text = text.gsub(/`([^`]+)`/) { "<code>#{$1}</code>" }
  text.gsub(/\[([^\]]+)\]\(([^)]+)\)/) { %(<a href="#{$2}">#{$1}</a>) }
end

def to_html(md)
  html = []
  in_list = false
  in_code = false
  para = []
  flush = lambda do
    html << "<p>#{inline(para.join(' '))}</p>" unless para.empty?
    para.clear
  end
  md.each_line(chomp: true) do |line|
    if line.start_with?('```')
      flush.()
      html << (in_code ? '</pre>' : '<pre>')
      in_code = !in_code
      next
    end
    if in_code
      html << line
      next
    end
    if line.start_with?('- ')
      flush.()
      html << '<ul>' unless in_list
      in_list = true
      html << "<li>#{inline(line[2..])}</li>"
      next
    elsif in_list
      html << '</ul>'
      in_list = false
    end
    case line
    when /\A(#+)\s+(.*)/
      flush.()
      html << "<h#{$1.size}>#{inline($2)}</h#{$1.size}>"
    when ''
      flush.()
    else
      para << line
    end
  end
  flush.()
  html << '</ul>' if in_list
  html.join("\n")
end

rng = Random.new(3)
doc = make_doc(rng, 100)
html = to_html(doc)
freq = Hash.new(0)
doc.scan(/[a-z]+/) { |w| freq[w] += 1 }
top = freq.sort_by { |w, c| [-c, w] }.first(5)
puts html.lines.count
puts top.inspect
puts html.scan(/<(\w+)/).flatten.tally.sort.to_h.inspect
