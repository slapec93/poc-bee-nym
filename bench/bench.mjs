import { execFileSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const clusterDir = join(root, 'cluster')

const { values: args } = parseArgs({
  options: {
    mode: { type: 'string', default: 'baseline' },
    reps: { type: 'string', default: '5' },
    sizes: { type: 'string', default: '4096,65536,1048576' },
    timeout: { type: 'string', default: '300' },
    restart: { type: 'boolean', default: false },
    fork: { type: 'boolean', default: false },
    sandbox: { type: 'boolean', default: false },
    paid: { type: 'boolean', default: false },
    uploader: { type: 'string', default: '2' },
    downloader: { type: 'string', default: '6' },
    out: { type: 'string', default: join(root, 'results') },
  },
})

const nym = ['nym', 'edge'].includes(args.mode)
const route = args.uploader === '2' && args.downloader === '6' ? '' : `-up${args.uploader}-down${args.downloader}`
const flags = ['fork', 'sandbox', 'paid'].filter(f => args[f])
const label = [args.mode, ...flags].join('-') + route
const upArgs = [...(nym ? [args.mode] : []), ...flags]
const NODES = [1, 2, 3, 4, 5, 6].map(n => ({ n, api: `http://localhost:${21631 + 2 * n}` }))
const UPLOADER = NODES[Number(args.uploader) - 1]
const DOWNLOADER = NODES[Number(args.downloader) - 1]
const TIMEOUT_MS = Number(args.timeout) * 1000
const LOG_PATTERNS = {
  deadline_exceeded: /context deadline exceeded/,
  no_peer_found: /no peer found/,
  overdraft_sleep: /sleeping to refresh overdraft/,
  handshake_failed: /handshake/i,
  negotiate_failed: /failed to negotiate/,
  pushsync_failed: /pushsync.*(fail|error)|push.*failed/i,
}

const composeFiles = ['docker-compose.yml', ...upArgs.map(a => `docker-compose.${a}.yml`)]
const env = {
  ...process.env,
  COMPOSE_FILE: composeFiles.join(':'),
}
const sh = (cmd, cmdArgs, opts = {}) =>
  execFileSync(cmd, cmdArgs, { cwd: clusterDir, env, encoding: 'utf8', maxBuffer: 1 << 30, ...opts })
const compose = (...a) => sh('docker', ['compose', ...a])

const getJson = async (url, init) => {
  const res = await fetch(url, { signal: AbortSignal.timeout(30_000), ...init }).catch(e => {
    throw new Error(`${init?.method ?? 'GET'} ${url}: ${e.message}`)
  })
  if (!res.ok) throw new Error(`${init?.method ?? 'GET'} ${url}: ${res.status} ${await res.text()}`)
  return res.json()
}

const percentile = (xs, p) => {
  if (!xs.length) return null
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.round((s.length - 1) * p)]
}

function startCluster() {
  try { compose('down', '-t', '2', '--remove-orphans') } catch {}
  const out = sh(join(clusterDir, 'up.sh'), upArgs, { stdio: ['ignore', 'pipe', 'inherit'] })
  const m = out.match(/full mesh after (\d+)s/)
  return m ? Number(m[1]) : null
}

async function topology() {
  return Promise.all(NODES.map(async ({ n, api }) => {
    const t = await getJson(`${api}/topology`)
    return { node: n, connected: t.connected, population: t.population, depth: t.depth }
  }))
}

async function usableBatch() {
  const { stamps } = await getJson(`${UPLOADER.api}/stamps`)
  const usable = (stamps ?? []).find(s => s.usable)
  if (usable) return usable.batchID
  const { currentPrice, minimumValidityBlocks } = await getJson(`${UPLOADER.api}/chainstate`)
  const amount = BigInt(currentPrice) * BigInt(minimumValidityBlocks) * 2n
  const { batchID } = await getJson(`${UPLOADER.api}/stamps/${amount}/20`, { method: 'POST', signal: AbortSignal.timeout(120_000) })
  const deadline = Date.now() + 300_000
  while (Date.now() < deadline) {
    const res = await fetch(`${UPLOADER.api}/stamps/${batchID}`, { signal: AbortSignal.timeout(30_000) })
    if (res.ok && (await res.json()).usable) return batchID
    await new Promise(r => setTimeout(r, 2000))
  }
  throw new Error(`batch ${batchID} not usable after 300s`)
}

function containerNet() {
  const lines = sh('docker', ['stats', '--no-stream', '--format', '{{json .}}']).trim().split('\n')
  const unit = { B: 1, kB: 1e3, KB: 1e3, MB: 1e6, GB: 1e9, KiB: 1024, MiB: 1 << 20, GiB: 1 << 30 }
  const bytes = s => { const m = s.trim().match(/^([\d.]+)\s*([A-Za-z]+)$/); return m ? Number(m[1]) * (unit[m[2]] ?? 1) : 0 }
  const result = {}
  for (const l of lines) {
    const s = JSON.parse(l)
    if (!s.Name.startsWith('bee-nym-')) continue
    const [rx, tx] = s.NetIO.split('/').map(bytes)
    result[s.Name.replace(/^bee-nym-|-1$/g, '')] = { rx, tx, cpu: parseFloat(s.CPUPerc), mem: s.MemUsage.split('/')[0].trim() }
  }
  return result
}

function logSummary(since) {
  const logs = compose('logs', '--no-log-prefix', '--since', since, ...NODES.map(({ n }) => `bee-${n}`))
  const counts = Object.fromEntries(Object.keys(LOG_PATTERNS).map(k => [k, 0]))
  for (const line of logs.split('\n')) {
    if (line.includes('api access')) continue
    for (const [k, re] of Object.entries(LOG_PATTERNS)) if (re.test(line)) counts[k]++
  }
  return counts
}

function sidecarStats() {
  if (!nym) return null
  return Object.fromEntries(NODES.map(({ n }) => {
    const lines = compose('logs', '--no-log-prefix', `nym-${n}`).split('\n').filter(l => l.includes('"event":"stats"'))
    return [`nym-${n}`, lines.length ? JSON.parse(lines.at(-1)) : null]
  }))
}

function sidecarConns(since) {
  if (!nym) return null
  const logs = compose('logs', '--no-log-prefix', '--since', since, ...NODES.map(({ n }) => `nym-${n}`))
  const conns = logs.split('\n').filter(l => l.includes('"event":"conn"')).map(l => JSON.parse(l))
  const firstResponse = conns.map(c => c.first_response_ms).filter(v => v != null)
  return {
    total: conns.length,
    errored: conns.filter(c => c.error).length,
    empty_resets: conns.filter(c => c.error && c.tcp_to_mix === 0 && c.mix_to_tcp === 0).length,
    first_response_ms_p50: percentile(firstResponse, 0.5),
    first_response_ms_p90: percentile(firstResponse, 0.9),
  }
}

async function timed(fn) {
  const t = performance.now()
  try {
    const value = await fn()
    return { ok: true, ms: Math.round(performance.now() - t), value }
  } catch (e) {
    return { ok: false, ms: Math.round(performance.now() - t), error: String(e.cause?.message ?? e.message).slice(0, 200) }
  }
}

async function runOne(batch, size) {
  const data = randomBytes(size)
  const up = await timed(async () => {
    const res = await fetch(`${UPLOADER.api}/bytes`, {
      method: 'POST',
      body: data,
      headers: { 'swarm-postage-batch-id': batch, 'swarm-deferred-upload': 'false', 'content-type': 'application/octet-stream' },
      signal: AbortSignal.timeout(TIMEOUT_MS),
    })
    if (!res.ok) throw new Error(`HTTP ${res.status} ${await res.text()}`)
    return (await res.json()).reference
  })
  if (!up.ok) return { size, upload: up, download: { ok: false, ms: 0, error: 'skipped' } }

  const down = await timed(async () => {
    const res = await fetch(`${DOWNLOADER.api}/bytes/${up.value}`, { signal: AbortSignal.timeout(TIMEOUT_MS) })
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const got = Buffer.from(await res.arrayBuffer())
    if (!got.equals(data)) throw new Error(`content mismatch (${got.length}/${size} bytes)`)
  })
  return { size, ref: up.value, upload: up, download: down }
}

function summarize(runs) {
  const bySize = {}
  for (const r of runs) (bySize[r.size] ??= []).push(r)
  return Object.entries(bySize).map(([size, rs]) => {
    const stat = key => {
      const ok = rs.filter(r => r[key].ok).map(r => r[key].ms)
      return { ok: ok.length, n: rs.length, p50_ms: percentile(ok, 0.5), max_ms: ok.length ? Math.max(...ok) : null }
    }
    return { size: Number(size), upload: stat('upload'), download: stat('download') }
  })
}

const fmt = s => (s.p50_ms == null ? '-' : `${(s.p50_ms / 1000).toFixed(2)}s / ${(s.max_ms / 1000).toFixed(2)}s`)

async function main() {
  const startedAt = new Date().toISOString()
  const result = { mode: label, startedAt, config: { reps: Number(args.reps), sizes: args.sizes, timeout_s: Number(args.timeout) } }

  if (args.restart) {
    console.log(`starting ${label} cluster...`)
    result.mesh_seconds = startCluster()
    console.log(`full mesh after ${result.mesh_seconds}s`)
  }
  result.topology_before = await topology()

  const batch = await usableBatch()
  const netBefore = containerNet()
  const runsSince = new Date().toISOString()
  const runs = []
  for (const size of args.sizes.split(',').map(Number)) {
    for (let rep = 0; rep < Number(args.reps); rep++) {
      const r = await runOne(batch, size)
      runs.push({ rep, ...r })
      const status = x => (x.ok ? `${(x.ms / 1000).toFixed(2)}s` : `FAIL(${x.error})`)
      console.log(`size=${size} rep=${rep} upload=${status(r.upload)} download=${status(r.download)}`)
    }
  }
  const netAfter = containerNet()

  result.runs = runs
  result.summary = summarize(runs)
  result.topology_after = await topology()
  result.bee_log_counts = logSummary(runsSince)
  result.sidecar_conns = sidecarConns(runsSince)
  result.sidecar_stats = sidecarStats()
  result.net_delta = Object.fromEntries(Object.entries(netAfter).map(([k, v]) => [k, {
    rx: v.rx - (netBefore[k]?.rx ?? 0), tx: v.tx - (netBefore[k]?.tx ?? 0), cpu_now: v.cpu, mem: v.mem,
  }]))

  mkdirSync(args.out, { recursive: true })
  const file = join(args.out, `${label}-${startedAt.replace(/[:.]/g, '-')}.json`)
  writeFileSync(file, JSON.stringify(result, null, 2))

  console.log(`\n${label}: size | upload ok, p50/max | download ok, p50/max`)
  for (const s of result.summary) {
    console.log(`  ${s.size} | ${s.upload.ok}/${s.upload.n}, ${fmt(s.upload)} | ${s.download.ok}/${s.download.n}, ${fmt(s.download)}`)
  }
  console.log('bee log counts:', result.bee_log_counts)
  if (nym) console.log('sidecar conns:', result.sidecar_conns)
  console.log(`wrote ${file}`)
}

main().catch(e => { console.error(e); process.exit(1) })
