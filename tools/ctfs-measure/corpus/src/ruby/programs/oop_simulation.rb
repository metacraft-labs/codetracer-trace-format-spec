# Event-driven bank simulation exercising classes, inheritance, modules,
# method_missing, exceptions with retry/ensure, and observers.
module Auditable
  def audit_log = (@audit_log ||= [])
  def audit(msg) = audit_log << "#{self.class.name.downcase}:#{msg}"
end

class InsufficientFunds < StandardError
  attr_reader :shortfall
  def initialize(shortfall)
    @shortfall = shortfall
    super("short by #{shortfall}")
  end
end

class Account
  include Auditable
  attr_reader :id, :balance

  def initialize(id, balance = 0)
    @id = id
    @balance = balance
    @observers = []
  end

  def subscribe(&blk) = @observers << blk

  def deposit(amount)
    raise ArgumentError, 'non-positive deposit' unless amount.positive?
    @balance += amount
    notify(:deposit, amount)
  end

  def withdraw(amount)
    limit = available
    raise InsufficientFunds.new(amount - limit) if amount > limit
    @balance -= amount
    notify(:withdraw, amount)
  end

  def available = @balance
  def month_end; end

  def method_missing(name, *args)
    if name.to_s.start_with?('report_')
      "#{id}/#{name.to_s.delete_prefix('report_')}=#{send(name.to_s.delete_prefix('report_'))}"
    else
      super
    end
  end

  def respond_to_missing?(name, priv = false) = name.to_s.start_with?('report_') || super

  private

  def notify(kind, amount)
    audit("#{kind}:#{amount}")
    @observers.each { |o| o.call(self, kind, amount) }
  end
end

class Savings < Account
  RATE = 0.01
  def month_end = (@balance += (@balance * RATE).round)
end

class Checking < Account
  def initialize(id, balance = 0, overdraft: 500)
    super(id, balance)
    @overdraft = overdraft
  end

  def available = @balance + @overdraft
  def month_end = (@balance -= 5 if @balance < 1000)
end

class Bank
  def initialize(rng)
    @rng = rng
    @accounts = []
    @stats = Hash.new(0)
  end

  def open(kind, id)
    acct = kind == :savings ? Savings.new(id, 1000) : Checking.new(id, 200)
    acct.subscribe { |_a, k, amt| @stats[k] += amt }
    @accounts << acct
  end

  def transfer(from, to, amount)
    attempts = 0
    begin
      attempts += 1
      from.withdraw(amount)
      to.deposit(amount)
      @stats[:transfers] += 1
    rescue InsufficientFunds => e
      @stats[:failed] += 1
      amount -= e.shortfall
      retry if attempts < 2 && amount.positive?
    ensure
      @stats[:attempts] += attempts
    end
  end

  def run(days)
    days.times do |day|
      a, b = @accounts.sample(2, random: @rng)
      transfer(a, b, @rng.rand(1..400))
      @accounts.sample(random: @rng).deposit(@rng.rand(1..100)) if day % 3 == 0
      @accounts.each(&:month_end) if (day % 30).zero?
    end
    @stats
  end

  def summary = @accounts.map { |a| a.report_balance }
end

bank = Bank.new(Random.new(11))
8.times { |i| bank.open(i.even? ? :savings : :checking, "A#{i}") }
p bank.run(300)
puts bank.summary.first(3)
begin
  Account.new('x').deposit(-1)
rescue ArgumentError => e
  puts e.message
end
