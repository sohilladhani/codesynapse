# codesynapse — Growth Playbook

Audience: AI-assisted devs, Rust ecosystem, all developers, AI/infra engineers
Channels: X, Reddit, HN, YouTube/video
Existing: blog (unknown reach), LinkedIn (some following), X (50+ followers, dormant)
Angle: built it for myself — personal pain point
Goal: stars + real users + community in 90 days

---

## Core narrative

> "I kept losing context every time I asked my AI assistant about a new codebase.
> It would grep files, miss connections, hallucinate structure. So I built a graph of my code
> and gave it to the AI as 32 MCP tools. Now it answers architecture questions correctly."

Lead with this story — not the feature list. The feature list is for people already sold.
The story is for everyone else.

---

## Pre-launch (before making repo public)

- [ ] Record a 60-90 second demo video: real repo, real question, right answer, no cuts
  - Good question: "What handles auth token expiry in this Django app?" → AI finds `core_exception_handler` via dense embeddings, not grep
  - Bad question: "list all files" — looks like grep, not impressive
- [ ] Write the launch blog post (see template below) — have it ready to publish day 1
- [ ] Draft the HN Show HN comment (300-500 words) — have it ready
- [ ] Draft the Reddit r/rust post — different tone from HN
- [ ] Make sure GitHub repo looks clean: good README hero, demo GIF visible, install works

### Account setup & karma building (1–2 weeks before launch)
New HN accounts can't post Show HNs immediately. Build account credibility first.

**Target:** 50+ karma and a 2-week account age before posting.

**How to build karma genuinely:**

1. **Find relevant threads** — Use the HN API or browse `news.ycombinator.com` for stories about:
   - Rust, AI assistants, MCP, code intelligence, developer tools
   - Programming languages, build tools, static analysis
   - LLMs, embeddings, tree-sitter, language servers

2. **Post thoughtful comments** — Not "great post!" or "nice". Add value:
   - Share a relevant experience ("I ran into this too — what fixed it was...")
   - Ask a substantive question ("How does this compare to X approach? I'm curious about tradeoffs on Y.")
   - Correct a misconception gently with evidence
   - Add a technical detail the post missed

3. **Submit interesting links** — 2–3 links before the Show HN:
   - A good blog post about Rust, MCP, or code intelligence
   - An interesting open-source project in the same space
   - An essay about developer workflows or AI tooling

4. **What to avoid:**
   - Linking to codesynapse in comments (looks promotional — let the Show HN be the first mention)
   - Arguing or being negative — HN downvotes snark
   - Posting too fast (1-2 comments/day is plenty)
   - Generic agreement ("+1", "this")

5. **Threads that tend to get upvotes:**
   - Comments that share hard-won experience ("we tried this in prod and here's what broke")
   - Comments that explain *why* something works, not just what
   - Comments that cite numbers or benchmarks
   - Asking the right clarifying question that leads to a better discussion

**Expected timeline:** 5–7 days of light but genuine engagement gets you to 50+ karma. Don't rush it — rushed accounts get flagged.

---

### HN karma-building: Thread shortlist (real-time, Jun 23 2026)

These are active HN threads where you can write a valuable comment using your existing expertise. **Do not mention codesynapse.** Just be a helpful, knowledgeable community member.

#### Tier 1: Best fit for your expertise (comment today)

1. **Prompt Injection as Role Confusion** (202 pts, 102 comments)
   `https://news.ycombinator.com/item?id=48631888`
   - Paper about how LLMs treat role tags as part of the security model, which is fundamentally broken
   - **Angle:** This is directly relevant to MCP tool security. Comment about how structured, out-of-band context (like code graphs) avoids injection in ways that in-band prompting can't. The paper argues role tags are "formatting tricks" that became the security architecture — you can contrast that with tools that use typed, structured inputs instead of prompt manipulation.
   - **Draft:** *"The core issue here — role tags being in-band — is exactly why structured tools (like MCP tools that return typed data) are fundamentally harder to inject than prompt-based approaches. When the tool returns a struct, not text, there's no role tag to confuse. The security model shifts from 'keep the prompt clean' to 'validate the data.'"*

2. **GLM-5.2 – How to Run Locally** (441 pts, 193 comments)
   `https://news.ycombinator.com/item?id=48636377`
   - Hot thread about running frontier models locally. Discussions about hardware requirements, quantization, MoE offloading.
   - **Angle:** Comment on local-first architectures and what's possible when you keep data local. The thread has people discussing whether local models are viable for coding — you can chime in on the tradeoffs (privacy, latency, data never leaves your machine vs. capability gap).

3. **Show HN: Oak – Git alternative designed for agents** (196 pts, 167 comments)
   `https://news.ycombinator.com/item?id=48631726`
   - Directly adjacent space: building infra for AI agents. Lots of skepticism in the thread.
   - **Angle:** A nuanced take on what agents actually need from tooling that's different from humans. Git is human-first (diff-based, linear history, merge conflicts). Agents think in concepts, not line diffs. Instead of a new VCS, the real gap is semantic history — but you could offer a thoughtful perspective on where agent infra matters vs. where it's premature.

4. **The Coming Loop (Armin Ronacher)** (24 pts, 7 comments)
   `https://news.ycombinator.com/item?id=48643180`
   - Armin Ronacher (Flask, Jinja2, and prominent Rust figure) writing about the AI coding agent loop and the value of comprehension
   - **Angle:** Small thread — your comment will be seen. Armin is a respected figure in both Python and Rust communities. Comment thoughtfully on the tension between agent speed and human comprehension. This is exactly the problem codesynapse was built to address (giving context to AI so it produces better output), but discuss it generally.
   - **Draft:** *"The comprehension piece is what I keep coming back to. Speed without understanding just means you break things faster. What I've found is that giving the agent structured context — not just files but the relationships between them — produces output that actually makes sense on first read. The loop tightens because the agent spends less time guessing."*

#### Tier 2: Good secondary options

5. **Deno Desktop** (1076 pts, 387 comments)
   `https://news.ycombinator.com/item?id=48626137`
   - Massive thread about shipping desktop apps via web tech vs. compiled languages
   - **Angle:** The Rust native vs Electron tradeoff. You built something in Rust — comment on why native compilation matters for dev tools (startup time, memory, binary size). The ~10MB binary for codesynapse is a concrete counterpoint to Electron bloat.

6. **The text in Claude Code's "Extended Thinking" output** (310 pts, 213 comments)
   `https://news.ycombinator.com/item?id=48630535`
   - Discussion about AI reasoning transparency and summarized reasoning traces
   - **Angle:** How structured context vs. raw text affects reasoning quality. Or the gap between what AI says it's doing vs. what it actually does when analyzing code.

7. **Package Managers need global hooks** (28 pts, 45 comments)
   `https://news.ycombinator.com/item?id=48586767`
   - Developer tooling design discussion
   - **Angle:** Extensibility patterns in dev tools — how hooks/open architectures enable community plugins vs. monolithic design.

8. **Who Does What? Team Topologies for the Agentic Platform** (30 pts, 11 comments)
   `https://news.ycombinator.com/item?id=48640382`
   - Low engagement, easy to get noticed. Agent architecture discussion.

**Posting cadence:** 1 comment/day on Tier 1, then Tier 2. Space them out — 2-3 in one day looks like a spree.

---

## Launch day

### 1. HN Show HN (highest ROI)
Post as: `Show HN: codesynapse – a code graph so your AI stops hallucinating my codebase`

Comment template:
```
I was tired of my AI assistant guessing wrong about my code.

"What handles auth token expiry?" → It greps for "token", finds the wrong file, makes something up.

So I built codesynapse. It indexes your code into a real graph — callers, callees, class hierarchies, trait implementations — then exposes that graph as MCP tools. Your AI can now trace the actual call chain instead of guessing.

The hybrid search (BM25 + dense embeddings) was the real unlock. Queries like "what builds the 404 response payload" map to `core_exception_handler` — lexical grep never finds that.

**How it works:**
- Scans your repo via tree-sitter parsers (Rust, Python, TS/JS, Go, more)
- Builds a code graph with petgraph (call relations, inheritance, trait impls)
- Exposes 32 MCP tools: `codesynapse_context`, `codesynapse_blast_radius`, `codesynapse_hierarchy`, hybrid search, etc.
- Your AI calls them like any other tool

**Stack:** Rust binary, ~10MB (UPX), zero cloud dependencies, fully local.

**Works with:** Claude Code, Cursor, Windsurf, Codex, Kiro — anything that speaks MCP.

Links: [GitHub](https://github.com/sohilladhani/codesynapse) · [demo GIF](https://github.com/sohilladhani/codesynapse/blob/main/assets/demo.gif)

Ask me anything — graph extraction, embedding approach, tree-sitter config, or why Rust.
```

**Timing:** post Tuesday–Thursday, 8-10am ET. Don't post Friday or weekend.
Watch the thread for the first 2 hours. Reply to every comment.

### 2. X/Twitter thread ✅ posted 2026-06-24
Even with 50 followers — post it. Tag:
- @AnthropicAI (MCP ecosystem)
- @cursor_ai
- Anyone in the Rust community you follow

Draft tweets (all under 280 chars):

**Tweet 1 — Hook**
> I built a tool that gives your AI assistant a map of your entire codebase.
> No more guessing. No more hallucinations.
>
> codesynapse 🧵👇

**Tweet 2 — Problem**
> Every time I asked my AI "what handles auth token expiry?" it grepped for "token", found the wrong file, and made something up.
>
> The problem: LLMs navigate code via lexical search. That's like navigating a city with only street names and no map.

**Tweet 3 — Solution (attach assets/demo.gif as media)**
> So I built a code graph: class X → Y → Z. Then exposed it as MCP tools.
>
> Now your AI can trace call chains, find blast radius, and navigate hierarchies — instead of guessing.

**Tweet 4 — How it works**
> How it works:
> • Scans repos via tree-sitter (Rust, Python, TS, Go, more)
> • Builds a call graph + class hierarchy
> • 32 MCP tools: context, blast radius, hierarchy, hybrid search
> • Fully local Rust binary, ~10MB, no cloud API

**Tweet 5 — Install + tags**
> Install: cargo install codesynapse
> Or grab the binary from GitHub.
>
> Works with Claude Code, Cursor, Windsurf, Codex, Kiro.
>
> https://github.com/sohilladhani/codesynapse
>
> @AnthropicAI @cursor_ai

**Tweet 6 — Closing**
> Built this for myself because existing tools couldn't answer architecture questions
> about real codebases. Sharing in case you're hitting the same wall.

### 3. Reddit
**r/rust**: Lead with the Rust angle — Rust binary, tree-sitter parsers, petgraph, Model2Vec embeddings. Rust devs care about the implementation.
**r/MachineLearning** or **r/LocalLLaMA**: Lead with the MCP angle — code intelligence layer for AI agents, fully local.

#### r/LocalLLaMA draft

**Title:** `I embedded my entire codebase with a 16M param local model and gave it to Claude as 32 MCP tools — it now answers architecture questions correctly`

**Body:**

Tired of AI assistants grepping files and hallucinating code structure. Built codesynapse to fix it.

**The local model part:** Uses `potion-code-16M` — 16M parameters, ~62MB on disk, runs CPU-only. No API key, no Nomic, no OpenAI embeddings. Trained specifically on code so vocabulary-gap queries work: "what builds the 404 response payload" maps to `core_exception_handler` even though the function name has nothing to do with "404".

**The MCP part:** Indexes your repo into a call graph via tree-sitter + petgraph, then exposes it as 32 MCP tools. Claude/Cursor/Windsurf can call `codesynapse_find_callers("AuthMiddleware")` or `codesynapse_blast_radius("UserService")` directly. No more guessing structure from file reads.

**Hybrid search:** BM25 + dense embeddings with RRF fusion. Lexical handles exact names, dense handles concepts. Both run locally.

**Binary:** Rust, ~10MB UPX compressed. Install:

```bash
cargo install codesynapse
codesynapse setup --client claude   # auto-configures MCP
codesynapse module add myrepo ./path/to/repo
```

20+ languages via tree-sitter (Python, TS, Rust, Go, Java, C/C++, more).

https://github.com/sohilladhani/codesynapse

Happy to go deep on the embedding model choice, RRF tuning, or the graph extraction approach.
**r/programming**: Lead with the story — "I was frustrated by my AI losing context"

Post on different days, not all at once.

#### r/rust draft

**Title:** `codesynapse – I built 32 MCP tools around a code graph so my AI stops hallucinating my codebase`

**Body:**

Been frustrated by AI assistants that grep files and confidently get architecture questions wrong. "What handles auth token expiry?" → finds the wrong handler, makes something up.

The root problem: LLMs navigate code lexically. No concept of call chains, class hierarchies, or module ownership.

So I built codesynapse. It indexes your repo via tree-sitter, builds a petgraph call graph, then exposes it as MCP tools. Your AI can now call `codesynapse_blast_radius("AuthMiddleware")` or `codesynapse_find_callers("SecurityContextHolder")` instead of grepping.

**Stack:**
- Rust binary, ~10MB (UPX compressed)
- tree-sitter parsers for 20+ languages
- petgraph for the call graph + class hierarchy
- BM25 + potion-code-16M dense embeddings (16M params, ~62MB, CPU-only, no API key)
- sled for the on-disk graph store

**The hybrid search was the real unlock.** Queries like "what builds the 404 response payload" map to `core_exception_handler` — the function name has nothing to do with "404". Pure lexical search misses it. The dense embeddings (trained specifically on code) bridge the vocabulary gap.

32 tools total: context lookup, blast radius, pagerank, cycle detection, shortest path, hierarchy traversal, read-with-callees, and more.

Works with Claude Code, Cursor, Windsurf — anything that speaks MCP.

```bash
cargo install codesynapse
codesynapse setup --client claude
```

https://github.com/sohilladhani/codesynapse

Happy to talk tree-sitter grammar edge cases, the petgraph setup, or why I picked sled over rocksdb.

### 4. LinkedIn
Personal story post — longer form than X. LinkedIn favors narrative. Write it as:
"I spent 3 months building a tool to solve a problem that was costing me hours every week..."
End with: "Just open sourced it. Would love feedback."

### 5. Blog post
Publish your existing blog. Title options:
- "Why I built a code graph for my AI assistant"
- "32 MCP tools for code intelligence — what I learned building codesynapse"
- "The problem with AI context windows and a graph-based solution"

Structure:
1. The problem you faced (specific example — a real codebase, a real wrong answer)
2. What you tried first (grep-based tools, didn't work for X reason)
3. The insight (graph + hybrid search)
4. How it works (high level — graph extraction, MCP tools, hybrid search)
5. What surprised you building it (the dense embeddings made a bigger difference than the graph)
6. Install + link

---

## Week 1-4

### Community seeding (moderate engagement)
- Search X and Reddit for "MCP server code" "Claude Code context" "Cursor codebase" — reply to relevant posts with a genuine answer, mention codesynapse only if directly relevant
- Find GitHub issues in competitor repos (codegraph, semble) where users request features you have — don't spam, but if someone asks "does this support hybrid search?" you can mention yours does
- Answer questions on the MCP Discord / Anthropic Discord about code intelligence

### Keep momentum
- Post a "week 1" update: stats (installs via telemetry, GitHub stars, issues opened)
- Fix the first bugs that come in fast — response speed in the first week signals project health
- Add a CHANGELOG entry for the first patch

---

## Content pipeline (ongoing)

| Content | Platform | When |
|---------|----------|------|
| Demo video: blast radius on a real PR | YouTube + X | Week 2 |
| Blog: "How the hybrid search works" (BM25 + dense RRF) | Blog | Week 3 |
| Blog: "Building MCP tools in Rust" | Blog + r/rust | Week 4 |
| YouTube: full walkthrough from install to first query | YouTube | Week 4 |
| Blog: "What codegraph does well and where I went different" | Blog + HN | Month 2 |

The codegraph comparison post will generate the most discussion. Write it fairly —
acknowledge what codegraph does well, explain technical tradeoffs. Don't make it a hit piece.

---

## GitHub repo health signals

Stars are lagging indicators. Leading indicators that drive organic growth:
- **Issues responded to within 24h** — first-time contributors check this
- **Good first issues labeled** — drives contributors
- **CHANGELOG updated** on every release
- **README demo GIF works** — broken GIFs kill conversion

Pin these to the top of your todo list for the first month.

---

## 90-day targets

| Metric | Target | How |
|--------|--------|-----|
| GitHub stars | 200+ | HN launch + Reddit + X |
| Real installs (telemetry) | 50+ active | HN + blog traffic |
| Issues opened by others | 20+ | Means real usage |
| Contributors | 2+ | Good first issues, fast responses |
| Blog traffic | 1k+ views on launch post | HN front page = 5-20k views |

---

## Beyond Posts: Marketing Strategies

### Do immediately (free, high ROI)

**GitHub topics** — add to repo settings: `mcp`, `mcp-server`, `code-intelligence`, `rust`, `tree-sitter`, `llm`, `claude`, `cursor`. GitHub Explore and search surfaces repos by topic. Pin the repo on your profile.

**Awesome lists** — submit to:
- `punkpeye/awesome-mcp-servers` (highest traffic MCP list)
- `wong2/awesome-mcp-servers` (second most linked)
One-line PR each. People actively browse these when looking for MCP tools. Steady passive installs.

**MCP community Discord/Slack**
- Anthropic Discord: `#mcp-servers` channel ✅ posted 2026-06-24
- Cursor Discord: active devs who use MCP daily ✅ posted 2026-06-24
- r/ClaudeAI Discord
Don't spam. Post once, be genuine: "built this, would love feedback."

### Week 1

**Product Hunt** ✅ demo GIF ready · YouTube video uploaded · thumbnail ready
- Launch as "MCP server for code intelligence"
- Best day: Tuesday
- First 2 hours critical: coordinate upvotes from network
- PH has active AI tools audience, different from HN crowd

**Direct outreach to creators**
- Find YouTubers doing Claude Code / Cursor tutorials (10k-100k subs)
- DM them, offer to help set it up
- One video = 10x any Reddit post

**AI newsletters**
- TLDR AI, The Batch, Import AI
- One mention = thousands of subscribers who are exactly the target audience

### Month 1

**Comparison content**
- "codesynapse vs codegraph" blog post
- Fair, acknowledge tradeoffs, don't make it a hit piece
- People searching for alternatives land on it organically
- r/rust will discuss it; codegraph users will find it

**GitHub issue seeding**
- Watch issues in codegraph, semble repos
- When someone asks for a feature you have, reply genuinely

---

## What NOT to do

- Don't mass-post to every subreddit on day 1 — looks spammy, gets removed
- Don't lead with the feature list — nobody reads it; lead with the story
- Don't compare yourself to codegraph aggressively — Rust community is small, people talk
- Don't ignore issues — one ignored issue is 10 lost potential users who watched
- Don't obsess over stars — one active user who files bugs and PRs is worth 100 passive stars
