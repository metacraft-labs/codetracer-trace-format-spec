// TypeScript (erasable-syntax only, so Node's type stripping can run it):
// discrete-event simulation of a multi-server queue with typed
// interfaces, literal-type unions, generics, discriminated unions and a generic heap.
const Kind = { Arrival: 0, Departure: 1, Report: 2 } as const;

interface ArrivalEv { kind: typeof Kind.Arrival; time: number; customer: number }
interface DepartureEv { kind: typeof Kind.Departure; time: number; customer: number; server: number }
interface ReportEv { kind: typeof Kind.Report; time: number }
type Ev = ArrivalEv | DepartureEv | ReportEv;

class Heap<T> {
  private data: T[] = [];
  private less: (a: T, b: T) => boolean;
  constructor(less: (a: T, b: T) => boolean) { this.less = less; }
  get length(): number { return this.data.length; }
  push(x: T): void {
    const d = this.data;
    d.push(x);
    let i = d.length - 1;
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (!this.less(d[i], d[p])) break;
      [d[i], d[p]] = [d[p], d[i]];
      i = p;
    }
  }
  pop(): T | undefined {
    const d = this.data;
    if (d.length === 0) return undefined;
    const top = d[0];
    const last = d.pop() as T;
    if (d.length) {
      d[0] = last;
      let i = 0;
      while (true) {
        const l = 2 * i + 1, r = l + 1;
        let m = i;
        if (l < d.length && this.less(d[l], d[m])) m = l;
        if (r < d.length && this.less(d[r], d[m])) m = r;
        if (m === i) break;
        [d[i], d[m]] = [d[m], d[i]];
        i = m;
      }
    }
    return top;
  }
}

class Rng {
  private s: number;
  constructor(s: number) { this.s = s; }
  next(): number { this.s = (this.s * 16807) % 2147483647; return this.s / 2147483647; }
  exp(mean: number): number { return -mean * Math.log(1 - this.next()); }
}

interface Stats { served: number; totalWait: number; maxQueue: number; reports: number }

function simulate(customers: number, servers: number, seed: number): Stats {
  const rng = new Rng(seed);
  const events = new Heap<Ev>((a, b) => a.time < b.time);
  const busy: boolean[] = new Array(servers).fill(false);
  const queue: { customer: number; arrived: number }[] = [];
  const arrivedAt = new Map<number, number>();
  const stats: Stats = { served: 0, totalWait: 0, maxQueue: 0, reports: 0 };
  events.push({ kind: Kind.Arrival, time: 0, customer: 0 });
  events.push({ kind: Kind.Report, time: 50 });
  let ev: Ev | undefined;
  while ((ev = events.pop()) !== undefined) {
    switch (ev.kind) {
      case Kind.Arrival: {
        arrivedAt.set(ev.customer, ev.time);
        const free = busy.indexOf(false);
        if (free >= 0) {
          busy[free] = true;
          events.push({ kind: Kind.Departure, time: ev.time + rng.exp(3), customer: ev.customer, server: free });
        } else {
          queue.push({ customer: ev.customer, arrived: ev.time });
          stats.maxQueue = Math.max(stats.maxQueue, queue.length);
        }
        if (ev.customer + 1 < customers) {
          events.push({ kind: Kind.Arrival, time: ev.time + rng.exp(1.1), customer: ev.customer + 1 });
        }
        break;
      }
      case Kind.Departure: {
        stats.served++;
        const nextC = queue.shift();
        if (nextC) {
          stats.totalWait += ev.time - nextC.arrived;
          events.push({ kind: Kind.Departure, time: ev.time + rng.exp(3), customer: nextC.customer, server: ev.server });
        } else {
          busy[ev.server] = false;
        }
        break;
      }
      case Kind.Report:
        stats.reports++;
        if (stats.served < customers) events.push({ kind: Kind.Report, time: ev.time + 50 });
        break;
    }
  }
  return stats;
}

const n = Number(process.argv[2] || 400);
for (const servers of [2, 3, 4]) {
  const s = simulate(n, servers, 2024 + servers);
  console.log(`servers=${servers} served=${s.served} avgWait=${(s.totalWait / s.served).toFixed(2)} maxQ=${s.maxQueue} reports=${s.reports}`);
}
