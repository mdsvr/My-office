import React, { useState, useEffect, useRef } from 'react';
import { 
  Sparkles, Play, Pause, Coffee, Terminal, GitPullRequest, 
  CheckCircle2, Clock, Cpu, Radio, Zap, 
  Shield, Search, Wrench, Send, Server,
  Plus, X, FolderGit2, GitMerge, Eye, Sliders, FileCode2
} from 'lucide-react';

// --- INITIAL DATA BASED ON SCREENSHOT ---
const INITIAL_AGENTS = [
  {
    id: 'scout',
    name: 'SCOUT',
    jpName: 'スカウト',
    model: 'Haiku',
    status: 'IDLE',
    tokens: 15,
    color: '#2dd4bf', // Teal
    darkColor: '#0d9488',
    role: 'Codebase Recon & Triage',
    specialty: 'Scans repo horizons with binoculars for incoming issues and anomalies.',
    taskText: '—',
    sfx: 'キョロ (LOOK)',
    accessory: 'antennae',
    row: 1,
    speech: 'Horizon is clear in MY OFFICE! No wild segfaults spotted~',
    promptSnippet: 'SELECT issue FROM repo WHERE severity >= "medium" ORDER BY created_at DESC;'
  },
  {
    id: 'guard',
    name: 'GUARD',
    jpName: 'ガード',
    model: 'Haiku',
    status: 'IDLE',
    tokens: 3,
    color: '#60a5fa', // Blue
    darkColor: '#2563eb',
    role: 'Security & Invariant Shield',
    specialty: 'Protects main branch integrity and blocks unsafe memory patterns.',
    taskText: '—',
    sfx: 'ピシッ (FIRM)',
    accessory: 'visor',
    row: 1,
    speech: 'Safety gates locked and loaded! 🛡️',
    promptSnippet: 'verify_sandbox_invariants(ast_tree, allow_network=False)'
  },
  {
    id: 'vet',
    name: 'VET',
    jpName: 'ベテラン',
    model: 'Haiku',
    status: 'IDLE',
    tokens: 3,
    color: '#a78bfa', // Purple
    darkColor: '#7c3aed',
    role: 'Issue Triage Squad (x3 Triplets)',
    specialty: 'Three synchronized chibi triplets with glasses who cross-examine bug reports.',
    taskText: '—',
    sfx: 'フムフム (HMMM)',
    accessory: 'glasses',
    row: 1,
    isSquad: true, // Renders 3 cute purple triplet bots side-by-side with VET under the middle desk!
    speech: 'Triplet sync at 100%! Ready to cross-examine specs.',
    promptSnippet: 'consensus_vote([vet_1.analyze(bug), vet_2.analyze(bug), vet_3.analyze(bug)])'
  },
  {
    id: 'watcher',
    name: 'WATCHER',
    jpName: 'ウォッチャー',
    model: 'Haiku',
    status: 'IDLE',
    tokens: 0,
    color: '#38bdf8', // Sky Blue
    darkColor: '#0284c7',
    role: 'Telemetry & Runtime Monitor',
    specialty: 'Listens to studio lo-fi beats while monitoring stack traces and stdout.',
    taskText: '—',
    sfx: 'ジーー (STARE)',
    accessory: 'headphones',
    row: 1,
    speech: 'Vibing to MY OFFICE lo-fi & watching stdout streams~ ♪',
    promptSnippet: 'tail -f /var/log/my-office/pytest-runner.stream'
  },
  {
    id: 'hunter',
    name: 'HUNTER',
    jpName: 'ハンター',
    model: 'Sonnet',
    status: 'IDLE',
    tokens: 0,
    color: '#d6b485', // Warm Tan / Safari Hat
    darkColor: '#a16207',
    role: 'Root-Cause Detective',
    specialty: 'Tracks elusive edge-cases across AST trees with keen detective intuition.',
    taskText: '—',
    sfx: 'ギラッ (GLINT)',
    accessory: 'beret',
    row: 2,
    speech: 'Give me a stack trace and I will find the culprit line!',
    promptSnippet: 'ast.walk(module) -> locate_faulty_binary_op(node="BinOp")'
  },
  {
    id: 'repro',
    name: 'REPRO',
    jpName: 'リプロ',
    model: 'Haiku',
    status: 'IDLE',
    tokens: 31,
    color: '#f87171', // Coral Red
    darkColor: '#dc2626',
    role: 'Test Case Sketch Artist',
    specialty: 'Draws minimal failing test scripts with a magnifying glass that catch bugs red-handed.',
    taskText: '[local] takes Silvertree2010/ao-...',
    sfx: 'カリカリ (SKETCH)',
    accessory: 'magnifier',
    row: 2,
    speech: 'Reproduced ao-testbed#6 in 0.4s! Look at this failing assert!',
    promptSnippet: 'def test_mean_small(): assert mean([2, 4]) == 3.0 # Fails with 2.0!'
  },
  {
    id: 'fixer',
    name: 'FIXER',
    jpName: 'フィクサー',
    model: 'Sonnet',
    status: 'IDLE',
    tokens: 501,
    color: '#fb923c', // Orange
    darkColor: '#ea580c',
    role: 'Lead Manga Inker & Patcher',
    specialty: 'Rewrites broken functions with clean, expressive surgical patches.',
    taskText: 'done: Fix mean() dividing by len(xs...',
    sfx: 'シュババ (INKING!)',
    accessory: 'headband',
    row: 2,
    speech: 'Phew! 501k tokens of pure surgical refactoring in MY OFFICE! ✍️',
    promptSnippet: '- return sum(xs) / (len(xs) + 1)\n+ return sum(xs) / len(xs)'
  },
  {
    id: 'ci',
    name: 'CI',
    jpName: 'シーアイ',
    model: 'no model',
    status: 'IDLE',
    tokens: 0,
    color: '#4ade80', // Green
    darkColor: '#16a34a',
    role: 'Automated Pytest Shrine',
    specialty: 'Runs deterministic test suites inside the local ME server rack.',
    taskText: 'green after 0 min',
    sfx: 'ピコン (DING!)',
    accessory: 'sprout',
    row: 2,
    onRack: true,
    speech: 'All assertions GREEN after 0 min! ✨',
    promptSnippet: '. .venv/bin/activate && pytest -q tests/test_math.py'
  },
  {
    id: 'reviewer',
    name: 'REVIEWER',
    jpName: 'レビュアー',
    model: 'Haiku',
    status: 'IDLE',
    tokens: 5,
    color: '#f472b6', // Pink / Magenta
    darkColor: '#db2777',
    role: 'Chief Manga Editor',
    specialty: 'Polishes code style with an oversized correction pen and stamps approval.',
    taskText: 'ok',
    sfx: 'ビシッ (STAMP!)',
    accessory: 'pen',
    row: 2,
    speech: 'Linework is clean! Proofread & approved: ok! 🌸',
    promptSnippet: 'LGTM! Zero regressions, clean diff, invariants preserved.'
  },
  {
    id: 'submit',
    name: 'SUBMIT',
    jpName: 'サブミット',
    model: 'no model',
    status: 'IDLE',
    tokens: 0,
    color: '#34d399', // Emerald Green
    darkColor: '#059669',
    role: 'Publisher & PR Courier',
    specialty: 'Packages verified commits and dispatches Pull Requests to GitHub.',
    taskText: 'PR opened: https://github.com...',
    sfx: 'ドーン! (SHIP!)',
    accessory: 'cap',
    row: 2,
    speech: 'Fresh manuscript delivered straight to GitHub! 📦',
    promptSnippet: 'gh pr create --title "Fix mean() dividing by len(xs) + 1" --body "Verified by MY OFFICE"'
  }
];

const INITIAL_QUEUE = [
  {
    id: 'ao-testbed#6',
    title: 'mean() is too small',
    url: 'https://github.com/Silvertree2010/ao-testbed/pull/6',
    status: 'DONE'
  },
  {
    id: 'ao-testbed#4',
    title: 'clamp() returns the lower bound for v...',
    url: 'https://github.com/Silvertree2010/ao-testbed/pull/4',
    status: 'DONE'
  }
];

const INITIAL_PRS = [
  {
    id: 'ao-testbed#7',
    title: 'Fix mean() dividing by len(xs) + 1',
    status: 'OPEN',
    agentsInvolved: ['scout', 'repro', 'fixer', 'ci', 'reviewer', 'submit'],
    file: 'src/ao_testbed/stats.py',
    diffRemoved: '    return sum(xs) / (len(xs) + 1)',
    diffAdded: '    return sum(xs) / len(xs)',
    summary: 'Removes off-by-one denominator `+ 1` in arithmetic mean calculation.'
  },
  {
    id: 'ao-testbed#5',
    title: 'Return upper bound from clamp',
    status: 'OPEN',
    agentsInvolved: ['hunter', 'repro', 'fixer', 'reviewer', 'submit'],
    file: 'src/ao_testbed/math_utils.py',
    diffRemoved: '    if val > hi:\n        return lo',
    diffAdded: '    if val > hi:\n        return hi',
    summary: 'Fixes copy-paste bug where values exceeding `hi` returned `lo` instead of `hi`.'
  }
];

const INITIAL_LOGS = [
  { time: '01:14', agent: 'Submit', color: '#16a34a', text: 'PR opened: https://github.com/Silvertree2010/ao-testbed/pull/7' },
  { time: '01:13', agent: 'Submit', color: '#16a34a', text: 'default branch unchanged, proofs still valid' },
  { time: '01:13', agent: 'Submit', color: '#16a34a', text: 'pre-push gate for Silvertree2010/ao-testbed#6 passed' },
  { time: '01:13', agent: 'Repro', color: '#dc2626', text: '[local] takes Silvertree2010/ao-testbed#6' },
  { time: '01:13', agent: 'Submit', color: '#16a34a', text: 'Silvertree2010/ao-testbed#6 is ready and verified' },
  { time: '01:13', agent: 'Submit', color: '#16a34a', text: 'default branch unchanged, proofs still valid' },
  { time: '01:13', agent: 'Submit', color: '#16a34a', text: 'pre-push gate for Silvertree2010/ao-testbed#4 passed' },
  { time: '01:13', agent: 'Reviewer', color: '#db2777', text: 'ok' },
  { time: '01:13', agent: 'CI', color: '#059669', text: 'green after 0 min' },
  { time: '01:13', agent: 'CI', color: '#059669', text: '. .venv/bin/activate && pytest running for ao-testbed' }
];

const SAMPLE_BUG_SCENARIOS = [
  {
    issueId: 'ao-testbed#8',
    issueTitle: 'fibonacci(0) raises IndexError on empty cache',
    prId: 'ao-testbed#9',
    prTitle: 'Guard base-case n <= 0 in fibonacci()',
    reproTask: '[local] takes Silvertree2010/ao-testbed#8',
    fixerTask: 'done: Guard base-case n <= 0 in fib...',
    file: 'src/ao_testbed/seq.py',
    diffRemoved: '    if n == 1:\n        return 1',
    diffAdded: '    if n <= 0:\n        return 0\n    if n == 1:\n        return 1',
    summary: 'Handles zero and negative inputs cleanly before indexing cache.'
  },
  {
    issueId: 'ao-testbed#10',
    issueTitle: 'matrix_transpose() drops last column on non-square',
    prId: 'ao-testbed#11',
    prTitle: 'Use zip(*matrix) bounds in transpose()',
    reproTask: '[local] takes Silvertree2010/ao-testbed#10',
    fixerTask: 'done: Fix col range in matrix_trans...',
    file: 'src/ao_testbed/linalg.py',
    diffRemoved: '    return [[row[i] for row in m] for i in range(len(m))]',
    diffAdded: '    return [list(col) for col in zip(*m)]',
    summary: 'Iterates over full column width instead of row count.'
  },
  {
    issueId: 'ao-testbed#12',
    issueTitle: 'parse_semver() fails when pre-release has hyphen',
    prId: 'ao-testbed#13',
    prTitle: 'Split semver on first hyphen only',
    reproTask: '[local] takes Silvertree2010/ao-testbed#12',
    fixerTask: 'done: Split maxsplit=1 in semver...',
    file: 'src/ao_testbed/version.py',
    diffRemoved: '    core, pre = version_str.split("-")',
    diffAdded: '    core, pre = version_str.split("-", 1)',
    summary: 'Limits hyphen split to `maxsplit=1` so multi-hyphen pre-releases parse properly.'
  }
];

// --- CUSTOM CHIBI MANGA BOT SVG COMPONENT ---
const ChibiBotDesk = ({ 
  agent, 
  labelOverride, 
  isSelected, 
  isHighlighted, 
  onClick, 
  matchaBoost,
  themeMode,
  subIndex = 0 
}) => {
  const isWorking = agent.status === 'WORKING';
  const [hovered, setHovered] = useState(false);

  // Subtle personality variation for VET triplets
  const eyeStyle = subIndex === 1 ? 'happy' : subIndex === 2 ? 'wink' : 'normal';

  return (
    <div 
      onClick={() => onClick(agent)}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      className={`relative group cursor-pointer select-none flex flex-col items-center transition-transform duration-200 ${
        isWorking ? 'animate-bounce' : 'hover:-translate-y-1.5'
      }`}
      style={{ zIndex: hovered || isSelected || isWorking ? 30 : 10 }}
    >
      {/* Floating Speech / SFX Bubble when hovered, working, or selected */}
      {(hovered || isWorking || isSelected || matchaBoost) && (
        <div 
          className="absolute -top-14 left-1/2 -translate-x-1/2 whitespace-nowrap px-2.5 py-1 rounded-xl border-2 border-[#1a1a1a] bg-white text-[#1a1a1a] text-[10px] font-bold shadow-[2px_2px_0px_#1a1a1a] flex items-center gap-1 z-40"
        >
          {matchaBoost ? (
            <span className="text-emerald-600 flex items-center gap-1">🍵 *SLURP!* +100% KAWAII! ✨</span>
          ) : isWorking ? (
            <span className="text-rose-600 flex items-center gap-1">⚡ {agent.sfx}</span>
          ) : (
            <span>💬 {agent.sfx}</span>
          )}
          <div className="absolute -bottom-1.5 left-1/2 -translate-x-1/2 w-2.5 h-2.5 bg-white border-r-2 border-b-2 border-[#1a1a1a] rotate-45"></div>
        </div>
      )}

      {/* Floating Manga Emotion Badge */}
      <div className="absolute -top-3 -right-2 pointer-events-none z-20">
        {isWorking && (
          <span className="inline-block px-1.5 py-0.5 text-[9px] font-black bg-yellow-300 text-[#1a1a1a] border-2 border-[#1a1a1a] rounded rotate-12 shadow-[1px_1px_0px_#1a1a1a] animate-pulse">
            カリカリ!
          </span>
        )}
        {!isWorking && matchaBoost && (
          <span className="text-sm animate-ping">💖</span>
        )}
        {!isWorking && !matchaBoost && isHighlighted && (
          <span className="inline-block px-1 py-0.5 text-[9px] font-black bg-pink-300 text-[#1a1a1a] border border-[#1a1a1a] rounded -rotate-6">
            ★ PR
          </span>
        )}
      </div>

      {/* Selection / Highlight Halo */}
      {(isSelected || isHighlighted) && (
        <div 
          className="absolute inset-0 -m-1.5 rounded-xl border-2 border-dashed pointer-events-none animate-pulse"
          style={{ borderColor: isSelected ? '#ec4899' : '#eab308' }}
        />
      )}

      {/* SVG Illustration of Monitor + Chibi Animator Bot + Desk */}
      <svg width="92" height="84" viewBox="0 0 96 86" className="overflow-visible drop-shadow-[2px_2px_0px_rgba(26,26,26,0.35)]">
        {/* Speed lines behind active agent */}
        {isWorking && (
          <g stroke="#f43f5e" strokeWidth="1.8" strokeLinecap="round">
            <line x1="6" y1="14" x2="18" y2="22" />
            <line x1="90" y1="14" x2="78" y2="22" />
            <line x1="48" y1="1" x2="48" y2="10" />
          </g>
        )}

        {/* Back Monitor (Left side of desk, matching screenshot) */}
        <g transform="translate(10, 22)">
          <rect x="10" y="24" width="6" height="6" fill="#4b5563" stroke="#1a1a1a" strokeWidth="2" />
          <rect x="6" y="28" width="14" height="3" rx="1" fill="#374151" stroke="#1a1a1a" strokeWidth="2" />
          <rect x="0" y="4" width="26" height="21" rx="3" fill="#1f2937" stroke="#1a1a1a" strokeWidth="2.2" />
          <rect x="3" y="7" width="20" height="15" rx="1.5" fill={isWorking ? '#fef08a' : '#111827'} />
          <line x1="6" y1="11" x2="16" y2="11" stroke={agent.color} strokeWidth="2" strokeLinecap="round" />
          <line x1="6" y1="15" x2="20" y2="15" stroke={isWorking ? '#ef4444' : '#60a5fa'} strokeWidth="1.8" strokeLinecap="round" />
          <line x1="6" y1="19" x2="13" y2="19" stroke="#9ca3af" strokeWidth="1.8" strokeLinecap="round" />
        </g>

        {/* Chibi Character Body/Head (Right of monitor, sitting at desk) */}
        <g transform={`translate(35, ${isWorking ? 15 : 18})`}>
          {/* Little Chibi Ears / Side Bolts */}
          <rect x="-3" y="18" width="4" height="7" rx="1.5" fill={agent.color} stroke="#1a1a1a" strokeWidth="2" />
          <rect x="39" y="18" width="4" height="7" rx="1.5" fill={agent.color} stroke="#1a1a1a" strokeWidth="2" />

          {/* Main Chibi Boxy-Rounded Head */}
          <rect 
            x="0" 
            y="8" 
            width="40" 
            height="27" 
            rx="6" 
            fill={agent.color} 
            stroke="#1a1a1a" 
            strokeWidth="2.5" 
          />

          {/* Manga Screentone Highlight on Forehead */}
          <path d="M 5 11 H 35" stroke="#ffffff" strokeWidth="2" strokeLinecap="round" opacity="0.7" />

          {/* Kawaii Blush Cheeks */}
          <ellipse cx="7" cy="26" rx="3" ry="1.6" fill="#fb7185" opacity="0.75" />
          <ellipse cx="33" cy="26" rx="3" ry="1.6" fill="#fb7185" opacity="0.75" />

          {/* Expressive Manga Eyes */}
          {isWorking ? (
            <g stroke="#1a1a1a" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" fill="none">
              <path d="M 8 19 L 13 22 L 8 25" />
              <path d="M 32 19 L 27 22 L 32 25" />
            </g>
          ) : eyeStyle === 'happy' || matchaBoost ? (
            <g stroke="#1a1a1a" strokeWidth="2.2" strokeLinecap="round" fill="none">
              <path d="M 7 23 Q 11 18 15 23" />
              <path d="M 25 23 Q 29 18 33 23" />
            </g>
          ) : eyeStyle === 'wink' ? (
            <g>
              <circle cx="11" cy="22" r="3.2" fill="#1a1a1a" />
              <circle cx="10" cy="20.8" r="1.2" fill="#ffffff" />
              <path d="M 25 22 Q 29 18 33 22" stroke="#1a1a1a" strokeWidth="2.2" strokeLinecap="round" fill="none" />
            </g>
          ) : (
            <g>
              <circle cx="11" cy="22" r="3.4" fill="#1a1a1a" />
              <circle cx="9.8" cy="20.6" r="1.3" fill="#ffffff" />
              <circle cx="12.2" cy="23.2" r="0.6" fill="#ffffff" />

              <circle cx="29" cy="22" r="3.4" fill="#1a1a1a" />
              <circle cx="27.8" cy="20.6" r="1.3" fill="#ffffff" />
              <circle cx="30.2" cy="23.2" r="0.6" fill="#ffffff" />
            </g>
          )}

          {/* Cute Mouth */}
          {isWorking ? (
            <rect x="17" y="25" width="6" height="4" rx="2" fill="#f43f5e" stroke="#1a1a1a" strokeWidth="1.5" />
          ) : (
            <path d="M 16 26 Q 18 28 20 26 Q 22 28 24 26" stroke="#1a1a1a" strokeWidth="1.8" fill="none" strokeLinecap="round" />
          )}

          {/* --- CHARACTER SPECIFIC ACCESSORIES --- */}
          {agent.accessory === 'antennae' && (
            <g>
              <line x1="20" y1="8" x2="20" y2="0" stroke="#1a1a1a" strokeWidth="2.2" />
              <circle cx="20" cy="0" r="3" fill="#fef08a" stroke="#1a1a1a" strokeWidth="1.8" />
            </g>
          )}

          {agent.accessory === 'visor' && (
            <g>
              <rect x="4" y="13" width="32" height="5" rx="2" fill="#fef08a" stroke="#1a1a1a" strokeWidth="1.8" />
            </g>
          )}

          {agent.accessory === 'glasses' && (
            <g stroke="#1a1a1a" strokeWidth="1.8" fill="rgba(255,255,255,0.42)">
              <rect x="4" y="17" width="13" height="9" rx="2" />
              <rect x="23" y="17" width="13" height="9" rx="2" />
              <line x1="17" y1="21" x2="23" y2="21" />
            </g>
          )}

          {agent.accessory === 'headphones' && (
            <g>
              <path d="M -2 20 C -2 3, 42 3, 42 20" stroke="#1a1a1a" strokeWidth="3.5" fill="none" />
              <rect x="-5" y="15" width="6" height="12" rx="3" fill="#ffffff" stroke="#1a1a1a" strokeWidth="2" />
              <rect x="39" y="15" width="6" height="12" rx="3" fill="#ffffff" stroke="#1a1a1a" strokeWidth="2" />
            </g>
          )}

          {agent.accessory === 'beret' && (
            <g>
              <ellipse cx="20" cy="7" rx="18" ry="5.5" fill="#b45309" stroke="#1a1a1a" strokeWidth="2" />
              <path d="M 20 1 L 20 -2" stroke="#1a1a1a" strokeWidth="2.5" strokeLinecap="round" />
            </g>
          )}

          {agent.accessory === 'magnifier' && (
            <g transform="translate(36, 16)">
              <circle cx="7" cy="4" r="5.5" fill="#e0f2fe" stroke="#1a1a1a" strokeWidth="2" />
              <line x1="11" y1="8" x2="16" y2="13" stroke="#1a1a1a" strokeWidth="2.5" strokeLinecap="round" />
            </g>
          )}

          {agent.accessory === 'headband' && (
            <g>
              <rect x="0" y="11" width="40" height="4.5" fill="#ffffff" stroke="#1a1a1a" strokeWidth="1.8" />
              <circle cx="20" cy="13.2" r="2" fill="#ef4444" />
              <path d="M 40 12 L 47 9 M 40 14 L 46 16" stroke="#ffffff" strokeWidth="2.5" strokeLinecap="round" />
              <path d="M 40 12 L 47 9 M 40 14 L 46 16" stroke="#1a1a1a" strokeWidth="1.2" strokeLinecap="round" />
            </g>
          )}

          {agent.accessory === 'pen' && (
            <g transform="translate(40, 8) rotate(20)">
              <rect x="0" y="4" width="4" height="18" rx="1" fill="#f43f5e" stroke="#1a1a1a" strokeWidth="1.8" />
              <polygon points="0,4 4,4 2,-2" fill="#fef08a" stroke="#1a1a1a" strokeWidth="1.5" />
            </g>
          )}

          {agent.accessory === 'cap' && (
            <g>
              <path d="M 2 9 C 2 1, 38 1, 38 9 Z" fill="#10b981" stroke="#1a1a1a" strokeWidth="2" />
              <rect x="12" y="7" width="29" height="3.5" rx="1.5" fill="#059669" stroke="#1a1a1a" strokeWidth="1.8" />
            </g>
          )}

          {/* Sweat drop when working hard */}
          {isWorking && (
            <path 
              d="M 38 12 C 38 12, 42 17, 38 19 C 34 17, 38 12, 38 12 Z" 
              fill="#38bdf8" 
              stroke="#1a1a1a" 
              strokeWidth="1.4" 
            />
          )}

          {/* Little Chibi Hands typing on desk */}
          <circle cx="10" cy="35" r="3.5" fill={agent.color} stroke="#1a1a1a" strokeWidth="2" />
          <circle cx="30" cy="35" r="3.5" fill={agent.color} stroke="#1a1a1a" strokeWidth="2" />
        </g>

        {/* Studio Desk Surface (Matches the screenshot's dark desk with colored LED bar!) */}
        <g transform="translate(6, 51)">
          <rect x="0" y="0" width="82" height="18" rx="3" fill="#27272a" stroke="#1a1a1a" strokeWidth="2.5" />
          <line x1="3" y1="3" x2="79" y2="3" stroke="#52525b" strokeWidth="2" strokeLinecap="round" />
          <rect 
            x="10" 
            y="8" 
            width="62" 
            height="4.5" 
            rx="2" 
            fill={agent.color} 
            stroke="#1a1a1a" 
            strokeWidth="1.5" 
          />
          <rect x="6" y="18" width="6" height="6" fill="#18181b" stroke="#1a1a1a" strokeWidth="2" />
          <rect x="70" y="18" width="6" height="6" fill="#18181b" stroke="#1a1a1a" strokeWidth="2" />
        </g>
      </svg>

      {/* Agent Desk Nameplate (Supports invisible spacer so VET triplets align cleanly with VET under the middle desk!) */}
      {labelOverride === '' ? (
        <div className="mt-0.5 h-[22px]" />
      ) : (
        <div className="mt-0.5 flex items-center gap-1">
          <span 
            className={`font-mono text-[11px] font-black tracking-wider px-2 py-0.5 rounded border-2 border-[#1a1a1a] transition-colors ${
              isSelected 
                ? 'bg-[#1a1a1a] text-white' 
                : isWorking 
                ? 'bg-yellow-300 text-[#1a1a1a]' 
                : themeMode === 'midnight'
                ? 'bg-zinc-800 text-zinc-100'
                : 'bg-white text-[#1a1a1a]'
            }`}
            style={{ boxShadow: '2px 2px 0px #1a1a1a' }}
          >
            {labelOverride || agent.name}
          </span>
        </div>
      )}
    </div>
  );
};

// --- CUTE POTTED MANGA PLANT SVG ---
const MangaPlant = ({ label }) => {
  const [wateredCount, setWateredCount] = useState(0);
  const [sparkling, setSparkling] = useState(false);

  const handleWater = () => {
    setWateredCount((c) => c + 1);
    setSparkling(true);
    setTimeout(() => setSparkling(false), 900);
  };

  return (
    <div 
      onClick={handleWater}
      title="Click to water the MY OFFICE studio plant! 🌱"
      className={`cursor-pointer select-none flex flex-col items-center transition-transform ${
        sparkling ? 'scale-125 -rotate-6' : 'hover:scale-110'
      }`}
    >
      {sparkling && (
        <span className="text-[10px] font-black bg-emerald-200 text-[#1a1a1a] border border-[#1a1a1a] px-1.5 py-0.5 rounded -mb-1 animate-bounce shadow-[1px_1px_0px_#1a1a1a]">
          パァァ✨ +{wateredCount}
        </span>
      )}
      <svg width="38" height="44" viewBox="0 0 38 44">
        {/* Leaves */}
        <path d="M 19 19 C 11 11, 7 17, 15 22" fill="#4ade80" stroke="#1a1a1a" strokeWidth="2" />
        <path d="M 19 19 C 27 11, 31 17, 23 22" fill="#22c55e" stroke="#1a1a1a" strokeWidth="2" />
        <path d="M 19 21 C 15 7, 23 7, 19 21" fill="#86efac" stroke="#1a1a1a" strokeWidth="2" />
        {/* Blossoms when watered! */}
        {wateredCount > 0 && (
          <circle cx="19" cy="9" r="3.5" fill="#f472b6" stroke="#1a1a1a" strokeWidth="1.5" />
        )}
        {/* Cute Pot with face */}
        <polygon points="11,22 27,22 25,37 13,37" fill="#3f3f46" stroke="#1a1a1a" strokeWidth="2.2" />
        <rect x="9" y="19" width="20" height="4" rx="1" fill="#52525b" stroke="#1a1a1a" strokeWidth="2" />
        <circle cx="16" cy="29" r="1.2" fill="#ffffff" />
        <circle cx="22" cy="29" r="1.2" fill="#ffffff" />
        <path d="M 17.5 31 Q 19 32.5 20.5 31" stroke="#ffffff" strokeWidth="1.3" fill="none" />
      </svg>
      {label && <span className="text-[9px] font-mono font-bold opacity-60">{label}</span>}
    </div>
  );
};

// --- STUDIO BOOMBOX / LO-FI RADIO SVG (Matches the radio icon to the right of the office sign!) ---
const StudioBoombox = ({ playing, onToggle }) => {
  return (
    <div 
      onClick={onToggle}
      title="Click to toggle MY OFFICE Lo-Fi Hip-Hop Radio! 📻"
      className={`cursor-pointer select-none flex items-center gap-2.5 px-3 py-1.5 rounded-xl border-2 border-[#1a1a1a] bg-white text-[#1a1a1a] shadow-[3px_3px_0px_#1a1a1a] transition-transform hover:-translate-y-0.5 ${
        playing ? 'ring-2 ring-pink-400' : 'opacity-80'
      }`}
    >
      <div className="relative">
        <svg width="46" height="32" viewBox="0 0 46 32" className={playing ? 'animate-bounce' : ''}>
          {/* Antenna */}
          <line x1="10" y1="7" x2="26" y2="1" stroke="#1a1a1a" strokeWidth="2" strokeLinecap="round" />
          <circle cx="26" cy="1" r="2" fill="#f43f5e" />
          {/* Handle */}
          <path d="M 14 7 L 14 4 L 32 4 L 32 7" stroke="#1a1a1a" strokeWidth="2" fill="none" />
          {/* Boombox Body */}
          <rect x="3" y="7" width="40" height="22" rx="3" fill="#3f3f46" stroke="#1a1a1a" strokeWidth="2.2" />
          {/* Top tuner strip */}
          <rect x="7" y="10" width="32" height="4" rx="1" fill="#fef08a" stroke="#1a1a1a" strokeWidth="1.4" />
          <line x1="18" y1="10" x2="18" y2="14" stroke="#ef4444" strokeWidth="2" />
          {/* Speakers */}
          <circle cx="12" cy="21" r="5" fill="#18181b" stroke="#1a1a1a" strokeWidth="1.8" />
          <circle cx="12" cy="21" r="2" fill="#a78bfa" />
          <circle cx="34" cy="21" r="5" fill="#18181b" stroke="#1a1a1a" strokeWidth="1.8" />
          <circle cx="34" cy="21" r="2" fill="#a78bfa" />
          {/* Cassette deck center */}
          <rect x="19" y="17" width="8" height="7" rx="1" fill="#2dd4bf" stroke="#1a1a1a" strokeWidth="1.5" />
        </svg>
        {playing && (
          <span className="absolute -top-2 -right-2 text-xs font-black text-rose-500 animate-float">
            ♪♫
          </span>
        )}
      </div>
      <div className="text-left leading-tight">
        <div className="text-[9px] font-mono font-black uppercase tracking-wider text-pink-600">
          MY OFFICE FM
        </div>
        <div className="text-[11px] font-black">
          {playing ? 'Lo-Fi Beats ON' : 'Radio Paused'}
        </div>
      </div>
    </div>
  );
};

export default function App() {
  const [agents, setAgents] = useState(INITIAL_AGENTS);
  const [queue, setQueue] = useState(INITIAL_QUEUE);
  const [activeWaitTask, setActiveWaitTask] = useState(null);
  const [prs, setPrs] = useState(INITIAL_PRS);
  const [logs, setLogs] = useState(INITIAL_LOGS);
  const [logSearch, setLogSearch] = useState('');

  const [selectedAgentId, setSelectedAgentId] = useState('fixer');
  const [highlightedPrId, setHighlightedPrId] = useState(null);
  const [inspectedPr, setInspectedPr] = useState(null); // Opens Manga Diff Manuscript Modal
  const [activeHeaderModal, setActiveHeaderModal] = useState(null); // 'repos' | 'machines' | null
  const [selectedBlade, setSelectedBlade] = useState(0);

  const [themeMode, setThemeMode] = useState('paper'); // 'paper' | 'shonen' | 'midnight'
  const [lofiPlaying, setLofiPlaying] = useState(true);
  const [officePaused, setOfficePaused] = useState(false);
  const [matchaAgentId, setMatchaAgentId] = useState(null);

  // Simulation state
  const [isSimulating, setIsSimulating] = useState(false);
  const [simStepText, setSimStepText] = useState('');
  const [scenarioIdx, setScenarioIdx] = useState(0);
  const [clockStr, setClockStr] = useState('01:14');
  const [doneCount, setDoneCount] = useState(2);

  // Custom modal for adding a custom bug
  const [showNewBugModal, setShowNewBugModal] = useState(false);
  const [customBugTitle, setCustomBugTitle] = useState('');

  const timeoutsRef = useRef([]);

  useEffect(() => {
    return () => timeoutsRef.current.forEach(clearTimeout);
  }, []);

  useEffect(() => {
    const interval = setInterval(() => {
      if (!officePaused) {
        const now = new Date();
        const mins = String(now.getMinutes()).padStart(2, '0');
        const secs = String(now.getSeconds()).padStart(2, '0');
        setClockStr(`${mins}:${secs}`);
      }
    }, 5000);
    return () => clearInterval(interval);
  }, [officePaused]);

  const selectedAgent = agents.find((a) => a.id === selectedAgentId) || agents[0];
  const highlightedPr = prs.find((p) => p.id === highlightedPrId);

  const openPrCount = prs.filter((p) => p.status === 'OPEN').length;
  const mergedPrCount = prs.filter((p) => p.status === 'MERGED').length;

  const updateAgent = (id, patch) => {
    setAgents((prev) =>
      prev.map((ag) => (ag.id === id ? { ...ag, ...patch } : ag))
    );
  };

  const pushLog = (agentName, color, text) => {
    const now = new Date();
    const t = `01:${String(14 + (now.getSeconds() % 45)).padStart(2, '0')}`;
    setLogs((prev) => [{ time: t, agent: agentName, color, text }, ...prev.slice(0, 39)]);
  };

  const handleGiveMatcha = (agentId) => {
    setMatchaAgentId(agentId);
    const target = agents.find((a) => a.id === agentId);
    if (target) {
      pushLog(target.name, target.darkColor, `received a warm Matcha Latte in MY OFFICE 🍵 (+100 Morale!)`);
    }
    setTimeout(() => {
      setMatchaAgentId(null);
    }, 2600);
  };

  const handleChangeAgentModel = (agentId, newModel) => {
    updateAgent(agentId, { model: newModel });
    const target = agents.find((a) => a.id === agentId);
    if (target) {
      pushLog(target.name, target.darkColor, `switched model engine to ${newModel}`);
    }
  };

  const handleSoloAgentAction = (agent) => {
    updateAgent(agent.id, { status: 'WORKING' });
    pushLog(agent.name, agent.darkColor, `running solo diagnostic check (${agent.sfx})...`);
    setTimeout(() => {
      updateAgent(agent.id, { status: 'IDLE', tokens: agent.tokens + 2 });
      pushLog(agent.name, agent.darkColor, `diagnostic complete — all checks nominal!`);
    }, 1800);
  };

  const handleMergePr = (prId, e) => {
    if (e) e.stopPropagation();
    setPrs((prev) =>
      prev.map((p) => (p.id === prId ? { ...p, status: 'MERGED' } : p))
    );
    if (inspectedPr && inspectedPr.id === prId) {
      setInspectedPr((prev) => ({ ...prev, status: 'MERGED' }));
    }
    pushLog('Submit', '#059669', `merged Pull Request ${prId} into main branch! ドーン! 🎉`);
  };

  // Trigger full multi-agent bug-hunt simulation across MY OFFICE!
  const startBugHuntSimulation = (customTitle = null) => {
    if (isSimulating) return;
    setOfficePaused(false);
    setIsSimulating(true);
    setHighlightedPrId(null);

    const baseScenario = SAMPLE_BUG_SCENARIOS[scenarioIdx % SAMPLE_BUG_SCENARIOS.length];
    const scenario = customTitle
      ? {
          issueId: `ao-testbed#${14 + scenarioIdx}`,
          issueTitle: customTitle,
          prId: `ao-testbed#${15 + scenarioIdx}`,
          prTitle: `Fix: ${customTitle.slice(0, 34)}`,
          reproTask: `[local] takes Silvertree2010/ao-testbed#${14 + scenarioIdx}`,
          fixerTask: `done: ${customTitle.slice(0, 28)}...`,
          file: 'src/ao_testbed/core.py',
          diffRemoved: '    return process_unverified(data)',
          diffAdded: '    if not data:\n        return []\n    return process_verified(data)',
          summary: `Automated MY OFFICE patch resolving "${customTitle}".`
        }
      : baseScenario;

    setScenarioIdx((i) => i + 1);

    timeoutsRef.current.forEach(clearTimeout);
    timeoutsRef.current = [];

    const schedule = (ms, fn) => {
      const id = setTimeout(fn, ms);
      timeoutsRef.current.push(id);
    };

    // Step 1: Issue arrives in Queue & SCOUT wakes up
    setActiveWaitTask({ id: scenario.issueId, title: scenario.issueTitle });
    setSimStepText(`PANEL 1: SCOUT spotted ${scenario.issueId} in MY OFFICE!`);
    setSelectedAgentId('scout');
    updateAgent('scout', {
      status: 'WORKING',
      tokens: 18,
      taskText: `scanning ${scenario.issueId}`,
      speech: `New bug manuscript spotted: "${scenario.issueTitle}"! 🔭`
    });
    pushLog('Scout', '#0d9488', `detected new issue ${scenario.issueId}: ${scenario.issueTitle}`);

    // Step 2: VET Triplets & GUARD triage
    schedule(1800, () => {
      updateAgent('scout', { status: 'IDLE', taskText: '—' });
      updateAgent('vet', {
        status: 'WORKING',
        tokens: 6,
        taskText: `vetting ${scenario.issueId}`,
        speech: `Triplets confirming bug validity! 100% reproducible spec!`
      });
      updateAgent('guard', {
        status: 'WORKING',
        tokens: 5,
        taskText: 'checking sandbox',
        speech: `Sandbox permissions verified for ${scenario.issueId}! 🛡️`
      });
      setSelectedAgentId('vet');
      setSimStepText(`PANEL 2: VET Triplets & GUARD triaging ${scenario.issueId}...`);
      pushLog('Vet', '#7c3aed', `triage passed for ${scenario.issueId} (priority: HIGH)`);
    });

    // Step 3: HUNTER & REPRO reproduce the bug
    schedule(3600, () => {
      updateAgent('vet', { status: 'IDLE', taskText: '—' });
      updateAgent('guard', { status: 'IDLE', taskText: '—' });
      updateAgent('hunter', {
        status: 'WORKING',
        tokens: 14,
        taskText: `locating AST node`,
        speech: `Aha! Found the exact faulty expression in ${scenario.file}! 🔍`
      });
      updateAgent('repro', {
        status: 'WORKING',
        tokens: 39,
        taskText: scenario.reproTask,
        speech: `Sketching failing pytest script for ${scenario.issueId}! ✍️`
      });
      setSelectedAgentId('repro');
      setSimStepText(`PANEL 3: HUNTER & REPRO isolating failing test...`);
      pushLog('Repro', '#dc2626', scenario.reproTask);
    });

    // Step 4: FIXER inks the code patch!
    schedule(5600, () => {
      updateAgent('hunter', { status: 'IDLE', taskText: '—' });
      updateAgent('repro', { status: 'IDLE' });
      updateAgent('fixer', {
        status: 'WORKING',
        tokens: 542,
        taskText: `patching ${scenario.issueId}...`,
        speech: `MAXIMUM MANGA SPEED! Inking the surgical patch right now!! 🖊️🔥`
      });
      setSelectedAgentId('fixer');
      setSimStepText(`PANEL 4: FIXER (Sonnet) inking surgical patch...`);
      pushLog('Fixer', '#ea580c', `applying AST patch for ${scenario.issueId} (+41k tok)`);
    });

    // Step 5: CI runs pytest on the ME Server Rack
    schedule(7600, () => {
      updateAgent('fixer', {
        status: 'IDLE',
        taskText: scenario.fixerTask,
        speech: `Patch complete! Passing manuscript to ME CI shrine! ✨`
      });
      updateAgent('ci', {
        status: 'WORKING',
        taskText: 'pytest running...',
        speech: `Running .venv/bin/activate && pytest inside ME rack!`
      });
      setSelectedAgentId('ci');
      setSimStepText(`PANEL 5: CI Machine (ME) running unit tests...`);
      pushLog('CI', '#059669', `. .venv/bin/activate && pytest running for ${scenario.issueId}`);
    });

    // Step 6: REVIEWER stamps OK
    schedule(9400, () => {
      updateAgent('ci', {
        status: 'IDLE',
        taskText: 'green after 0 min',
        speech: `All tests GREEN in 0 min! Zero regressions! 🌱`
      });
      pushLog('CI', '#059669', `green after 0 min`);

      updateAgent('reviewer', {
        status: 'WORKING',
        tokens: 8,
        taskText: `reviewing ${scenario.prId}`,
        speech: `Checking invariants with my red brush... Stamp of Approval: ok! 🌸`
      });
      setSelectedAgentId('reviewer');
      setSimStepText(`PANEL 6: REVIEWER inspecting code quality...`);
      pushLog('Reviewer', '#db2777', `ok`);
    });

    // Step 7: SUBMIT publishes PR & moves Queue item to DONE
    schedule(11200, () => {
      updateAgent('reviewer', { status: 'IDLE', taskText: 'ok' });
      updateAgent('submit', {
        status: 'WORKING',
        taskText: `PR opened: ${scenario.prId}`,
        speech: `Dispatching Pull Request ${scenario.prId} from MY OFFICE to GitHub! 🚀`
      });
      setSelectedAgentId('submit');
      setSimStepText(`FINAL PANEL: SUBMIT opening Pull Request ${scenario.prId}! ドーン!`);

      pushLog('Submit', '#16a34a', `pre-push gate for Silvertree2010/${scenario.issueId} passed`);
      pushLog('Submit', '#16a34a', `PR opened: https://github.com/Silvertree2010/ao-testbed/pull/${scenario.prId.split('#')[1]}`);

      setActiveWaitTask(null);
      setQueue((prev) => [
        {
          id: scenario.issueId,
          title: scenario.issueTitle,
          url: `https://github.com/Silvertree2010/ao-testbed/pull/${scenario.issueId.split('#')[1]}`,
          status: 'DONE'
        },
        ...prev
      ]);

      setPrs((prev) => [
        {
          id: scenario.prId,
          title: scenario.prTitle,
          status: 'OPEN',
          agentsInvolved: ['scout', 'vet', 'repro', 'fixer', 'ci', 'reviewer', 'submit'],
          file: scenario.file,
          diffRemoved: scenario.diffRemoved,
          diffAdded: scenario.diffAdded,
          summary: scenario.summary
        },
        ...prev
      ]);
      setDoneCount((c) => c + 1);
    });

    schedule(12800, () => {
      updateAgent('submit', { status: 'IDLE' });
      setIsSimulating(false);
      setSimStepText('');
    });
  };

  // Theme palette classes
  const getThemeStyles = () => {
    if (themeMode === 'midnight') {
      return {
        pageBg: 'bg-[#111217] text-zinc-100',
        panelBg: 'bg-[#1a1c24] text-zinc-100',
        floorBg: '#14161d',
        dotColor: 'rgba(255,255,255,0.06)',
        cardBg: 'bg-[#222530]'
      };
    }
    if (themeMode === 'shonen') {
      return {
        pageBg: 'bg-white text-[#111111]',
        panelBg: 'bg-white text-[#111111]',
        floorBg: '#f4f4f5',
        dotColor: 'rgba(0,0,0,0.14)',
        cardBg: 'bg-zinc-50'
      };
    }
    return {
      pageBg: 'bg-[#fdfbf7] text-[#1a1a1a]',
      panelBg: 'bg-[#fffdf9] text-[#1a1a1a]',
      floorBg: '#f6f1e7',
      dotColor: 'rgba(26,26,26,0.085)',
      cardBg: 'bg-white'
    };
  };

  const theme = getThemeStyles();
  const row1Agents = agents.filter((a) => a.row === 1);
  const row2Agents = agents.filter((a) => a.row === 2 && !a.onRack);

  const filteredLogs = logs.filter(
    (l) =>
      l.agent.toLowerCase().includes(logSearch.toLowerCase()) ||
      l.text.toLowerCase().includes(logSearch.toLowerCase())
  );

  return (
    <div 
      className={`min-h-screen w-full ${theme.pageBg} font-sans transition-colors duration-300 pb-8 select-none`}
      style={{
        backgroundImage: `radial-gradient(${theme.dotColor} 1.25px, transparent 1.25px)`,
        backgroundSize: '12px 12px'
      }}
    >
      {/* Custom Styles for Comic Halftones, Speedlines, and Neo-Brutalist Manga Borders */}
      <style>{`
        @keyframes floatSlow {
          0%, 100% { transform: translateY(0px) rotate(0deg); }
          50% { transform: translateY(-5px) rotate(4deg); }
        }
        .animate-float {
          animation: floatSlow 2.5s ease-in-out infinite;
        }
        .manga-panel {
          border: 3px solid #1a1a1a;
          box-shadow: 4px 4px 0px #1a1a1a;
        }
        .manga-card {
          border: 2.5px solid #1a1a1a;
          box-shadow: 3px 3px 0px #1a1a1a;
        }
        .manga-btn {
          border: 2.5px solid #1a1a1a;
          box-shadow: 2.5px 2.5px 0px #1a1a1a;
          transition: all 0.12s ease;
        }
        .manga-btn:hover {
          transform: translate(-1px, -1px);
          box-shadow: 4px 4px 0px #1a1a1a;
        }
        .manga-btn:active {
          transform: translate(2px, 2px);
          box-shadow: 0.5px 0.5px 0px #1a1a1a;
        }
        .checker-floor {
          background-image: 
            linear-gradient(45deg, rgba(26,26,26,0.045) 25%, transparent 25%), 
            linear-gradient(-45deg, rgba(26,26,26,0.045) 25%, transparent 25%), 
            linear-gradient(45deg, transparent 75%, rgba(26,26,26,0.045) 75%), 
            linear-gradient(-45deg, transparent 75%, rgba(26,26,26,0.045) 75%);
          background-size: 32px 32px;
          background-position: 0 0, 0 16px, 16px -16px, -16px 0;
        }
        .checker-floor-dark {
          background-image: 
            linear-gradient(45deg, rgba(255,255,255,0.03) 25%, transparent 25%), 
            linear-gradient(-45deg, rgba(255,255,255,0.03) 25%, transparent 25%), 
            linear-gradient(45deg, transparent 75%, rgba(255,255,255,0.03) 75%), 
            linear-gradient(-45deg, transparent 75%, rgba(255,255,255,0.03) 75%);
          background-size: 32px 32px;
          background-position: 0 0, 0 16px, 16px -16px, -16px 0;
        }
      `}</style>

      {/* ==================== TOP GLOBAL HEADER BAR ("MY OFFICE") ==================== */}
      <header className={`sticky top-0 z-40 px-4 py-2.5 border-b-4 border-[#1a1a1a] ${theme.panelBg} flex flex-wrap items-center justify-between gap-3`}>
        {/* Left: MY OFFICE Title & Status Badges (Mirroring MY OFFICE | online | local | vet 0 · ready 0 · approve 0 · done 2) */}
        <div className="flex flex-wrap items-center gap-2.5">
          <div className="flex items-center gap-2 bg-[#1a1a1a] text-white px-3.5 py-1 rounded-md border-2 border-[#1a1a1a] shadow-[2px_2px_0px_#ec4899]">
            <Sparkles className="w-4 h-4 text-pink-400" />
            <span className="font-black tracking-widest text-sm sm:text-base">MY OFFICE</span>
            <span className="text-[10px] bg-pink-500 text-white px-1.5 py-0.2 rounded font-bold">マイ・オフィス</span>
          </div>

          {/* Online Stamp */}
          <span className="px-2.5 py-0.5 rounded-full text-xs font-black border-2 border-[#1a1a1a] bg-emerald-300 text-[#1a1a1a] shadow-[2px_2px_0px_#1a1a1a] flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-emerald-700 animate-ping" />
            {officePaused ? 'paused' : 'online'}
          </span>

          {/* Local Stamp */}
          <span className="px-2.5 py-0.5 rounded-full text-xs font-bold border-2 border-[#1a1a1a] bg-amber-100 text-[#1a1a1a] shadow-[2px_2px_0px_#1a1a1a]">
            local
          </span>

          {/* Pipeline Counter Strip */}
          <div className="hidden xl:flex items-center gap-2 px-3 py-1 rounded-md border-2 border-[#1a1a1a] bg-white text-[#1a1a1a] font-mono text-xs font-bold shadow-[2px_2px_0px_#1a1a1a]">
            <span>vet <strong className="text-purple-600">{isSimulating ? 1 : 0}</strong></span>
            <span>·</span>
            <span>ready <strong className="text-sky-600">0</strong></span>
            <span>·</span>
            <span>approve <strong className="text-pink-600">0</strong></span>
            <span>·</span>
            <span>done <strong className="text-emerald-600">{doneCount}</strong></span>
          </div>
        </div>

        {/* Right: Interactive Controls, Repos, Machines & Manga Theme Switcher */}
        <div className="flex flex-wrap items-center gap-2">
          {/* Run Full Pipeline CTA */}
          <button
            onClick={() => startBugHuntSimulation()}
            disabled={isSimulating}
            className={`manga-btn px-3 py-1.5 rounded-md font-black text-xs flex items-center gap-1.5 ${
              isSimulating
                ? 'bg-yellow-300 text-[#1a1a1a] cursor-wait'
                : 'bg-gradient-to-r from-pink-400 via-rose-400 to-amber-300 text-[#1a1a1a]'
            }`}
          >
            <Zap className="w-3.5 h-3.5 fill-current" />
            {isSimulating ? 'RUNNING PIPELINE...' : '⚡ Run Full Pipeline'}
          </button>

          {/* + New Issue Button */}
          <button
            onClick={() => setShowNewBugModal(true)}
            className="manga-btn px-2.5 py-1.5 rounded-md bg-sky-200 text-[#1a1a1a] font-black text-xs flex items-center gap-1"
            title="Dispatch your own custom bug into MY OFFICE"
          >
            <Plus className="w-3.5 h-3.5" /> New Issue
          </button>

          {/* Repos Button (From screenshot top-right) */}
          <button
            onClick={() => setActiveHeaderModal(activeHeaderModal === 'repos' ? null : 'repos')}
            className="manga-btn px-2.5 py-1.5 rounded-md bg-white text-[#1a1a1a] font-black text-xs flex items-center gap-1"
          >
            <FolderGit2 className="w-3.5 h-3.5 text-indigo-600" /> Repos
          </button>

          {/* Machines Button (From screenshot top-right) */}
          <button
            onClick={() => setActiveHeaderModal(activeHeaderModal === 'machines' ? null : 'machines')}
            className="manga-btn px-2.5 py-1.5 rounded-md bg-white text-[#1a1a1a] font-black text-xs flex items-center gap-1"
          >
            <Server className="w-3.5 h-3.5 text-emerald-600" /> Machines
          </button>

          {/* Ink / Paper Theme Selector */}
          <div className="flex items-center rounded-md border-2 border-[#1a1a1a] bg-white p-0.5 shadow-[2px_2px_0px_#1a1a1a]">
            {[
              { id: 'paper', label: '📜 Paper' },
              { id: 'shonen', label: '✒️ Ink B&W' },
              { id: 'midnight', label: '🌙 Night' }
            ].map((t) => (
              <button
                key={t.id}
                onClick={() => setThemeMode(t.id)}
                className={`px-2 py-1 text-[11px] font-black rounded transition-colors ${
                  themeMode === t.id
                    ? 'bg-[#1a1a1a] text-white'
                    : 'text-[#1a1a1a] hover:bg-zinc-100'
                }`}
              >
                {t.label}
              </button>
            ))}
          </div>

          {/* Pause Office Button (From screenshot top-right) */}
          <button
            onClick={() => setOfficePaused(!officePaused)}
            className={`manga-btn px-2.5 py-1.5 rounded-md text-xs font-black flex items-center gap-1 ${
              officePaused ? 'bg-rose-300 text-[#1a1a1a]' : 'bg-white text-[#1a1a1a]'
            }`}
          >
            {officePaused ? <Play className="w-3.5 h-3.5" /> : <Pause className="w-3.5 h-3.5" />}
            {officePaused ? 'Resume office' : 'Pause office'}
          </button>
        </div>
      </header>

      {/* ==================== TOP MANGA STRIP: 10 AGENT ROSTER CARDS ==================== */}
      <section className="px-4 pt-3">
        <div className="grid grid-cols-2 sm:grid-cols-5 lg:grid-cols-10 gap-2">
          {agents.map((ag) => {
            const isSelected = selectedAgentId === ag.id;
            const isWorking = ag.status === 'WORKING';
            const isInHighlightedPr = highlightedPr?.agentsInvolved?.includes(ag.id);

            return (
              <div
                key={ag.id}
                onClick={() => setSelectedAgentId(ag.id)}
                className={`manga-card rounded-lg p-2 cursor-pointer transition-all relative overflow-hidden flex flex-col justify-between min-h-[84px] ${
                  isWorking
                    ? 'bg-yellow-100 text-[#1a1a1a] -translate-y-0.5 ring-2 ring-rose-500'
                    : isSelected
                    ? 'bg-pink-50 text-[#1a1a1a] -translate-y-0.5'
                    : `${theme.cardBg}`
                }`}
              >
                {/* Top Colored Manga Strip Bar */}
                <div 
                  className="absolute top-0 left-0 right-0 h-1.5 border-b border-[#1a1a1a]"
                  style={{ backgroundColor: ag.color }}
                />

                {/* Header Row: Color Dot + Name + Model + Gear */}
                <div className="flex items-center justify-between gap-1 mt-1">
                  <div className="flex items-center gap-1.5 min-w-0">
                    <span
                      className="w-2.5 h-2.5 rounded-xs border border-[#1a1a1a] shrink-0"
                      style={{ backgroundColor: ag.color }}
                    />
                    <span className="font-black text-[11px] tracking-tight truncate">
                      {ag.name}
                    </span>
                  </div>
                  <span className="text-[10px] font-mono opacity-65 shrink-0 flex items-center gap-0.5">
                    {ag.model}
                    {ag.model !== 'no model' && <span className="text-[9px] opacity-60">⚙</span>}
                  </span>
                </div>

                {/* Middle Task Snippet */}
                <div className="my-1 text-[10px] font-mono leading-tight line-clamp-2 opacity-85">
                  {ag.taskText}
                </div>

                {/* Bottom Row: Status + Token Counter */}
                <div className="flex items-center justify-between text-[10px] font-mono pt-1 border-t border-dashed border-zinc-300/70">
                  <span
                    className={`font-black px-1 rounded ${
                      isWorking
                        ? 'bg-rose-500 text-white animate-pulse'
                        : 'opacity-60'
                    }`}
                  >
                    {ag.status}
                  </span>
                  <span className="font-bold opacity-75">
                    {ag.tokens > 0 ? `${ag.tokens}k tok` : ''}
                  </span>
                </div>

                {isInHighlightedPr && (
                  <span className="absolute bottom-1 right-1 w-2 h-2 rounded-full bg-pink-500" />
                )}
              </div>
            );
          })}
        </div>
      </section>

      {/* ==================== MAIN MANGA PAGE SPREAD (LEFT STUDIO FLOOR + RIGHT 3 PANELS) ==================== */}
      <main className="px-4 pt-3 grid grid-cols-1 lg:grid-cols-12 gap-4 items-start">
        
        {/* -------------------- LEFT 8 COLS: THE "MY OFFICE" MANGA STUDIO FLOOR -------------------- */}
        <div className="lg:col-span-8 flex flex-col gap-3">
          <div 
            className="manga-panel rounded-xl overflow-hidden relative"
            style={{ backgroundColor: theme.floorBg }}
          >
            {/* Comic Panel Top Narration Strip / Studio Signboard */}
            <div className={`p-3 sm:p-4 border-b-2 border-[#1a1a1a] flex flex-wrap items-center justify-between gap-3 ${
              themeMode === 'midnight' ? 'bg-zinc-900/90' : 'bg-[#fff9ed]'
            }`}>
              {/* Left Digital Clock (01:14 in screenshot) */}
              <div className="flex items-center gap-2.5">
                <div className="px-3 py-1.5 rounded-md border-2 border-[#1a1a1a] bg-[#1a1a1a] text-emerald-400 font-mono text-sm sm:text-base font-black tracking-widest shadow-[2px_2px_0px_#93c5fd]">
                  {clockStr}
                </div>
                <div className="hidden sm:flex flex-col">
                  <span className="text-[10px] font-black uppercase tracking-wider opacity-60">
                    PAGE 01 // CH. MY OFFICE
                  </span>
                  <span className="text-xs font-bold">Manga Animator Workbench</span>
                </div>
              </div>

              {/* Center Signboard: "MY OFFICE / Q 0 PR 2" (Replaces APOLLO OFFICE right in the middle!) */}
              <div className="px-5 py-1.5 rounded-lg border-2 border-[#1a1a1a] bg-[#14151a] text-white shadow-[3px_3px_0px_#f472b6] flex flex-col items-center min-w-[190px]">
                <div className="flex items-center justify-between w-full gap-3">
                  <span className="font-mono font-black tracking-widest text-xs sm:text-sm text-amber-200">
                    MY OFFICE
                  </span>
                  <span className="w-2.5 h-2.5 rounded-xs bg-emerald-400 inline-block animate-pulse" />
                </div>
                <div className="font-mono text-xs text-zinc-300 tracking-widest w-full text-left mt-0.5">
                  Q {activeWaitTask ? 1 : 0} &nbsp; PR {openPrCount}
                </div>
              </div>

              {/* Right Vintage Studio Boombox / Lo-Fi Radio SVG */}
              <StudioBoombox
                playing={lofiPlaying}
                onToggle={() => setLofiPlaying(!lofiPlaying)}
              />
            </div>

            {/* Active Simulation Narration Banner */}
            {simStepText && (
              <div className="bg-yellow-300 text-[#1a1a1a] border-b-2 border-[#1a1a1a] px-4 py-1.5 font-black text-xs flex items-center justify-between animate-pulse">
                <span className="flex items-center gap-2">
                  <Sparkles className="w-4 h-4 fill-current" />
                  {simStepText}
                </span>
                <span className="font-mono text-[11px] bg-[#1a1a1a] text-yellow-300 px-2 py-0.5 rounded">
                  MY OFFICE LIVE STORYBOARD
                </span>
              </div>
            )}

            {/* STUDIO CHECKERBOARD FLOOR AREA */}
            <div className={`p-4 sm:p-6 relative min-h-[435px] flex flex-col justify-between ${
              themeMode === 'midnight' ? 'checker-floor-dark' : 'checker-floor'
            }`}>
              {/* Manga Crop Marks & SFX */}
              <div className="pointer-events-none absolute top-2 left-2 text-[10px] font-black opacity-30 font-mono">
                + [ゴゴゴゴ...]
              </div>
              <div className="pointer-events-none absolute top-2 right-2 text-[10px] font-black opacity-30 font-mono">
                [MY OFFICE STUDIO] +
              </div>

              {/* FLOOR GRID: Left 8 Cols for Agent Desks + Right 4 Cols for MACHINES Server Shrine */}
              <div className="grid grid-cols-1 xl:grid-cols-12 gap-6 items-center my-auto">
                
                {/* --- AGENT DESKS AREA (2 ROWS FAITHFUL TO SCREENSHOT) --- */}
                <div className="xl:col-span-8 flex flex-col gap-9">
                  
                  {/* ROW 1: SCOUT, GUARD, VET (x3 Triplets with VET centered under middle desk!), WATCHER */}
                  <div>
                    <div className="flex items-center gap-2 mb-2">
                      <span className="text-[10px] font-black uppercase tracking-widest px-2 py-0.5 rounded bg-[#1a1a1a] text-white">
                        ROW 01 // RECON, GUARD & VET TRIPLETS
                      </span>
                      <div className="h-0.5 flex-1 bg-[#1a1a1a]/15" />
                    </div>

                    <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-6 gap-2 justify-items-center items-end pt-4">
                      {/* SCOUT */}
                      <ChibiBotDesk
                        agent={row1Agents[0]}
                        isSelected={selectedAgentId === 'scout'}
                        isHighlighted={highlightedPr?.agentsInvolved?.includes('scout')}
                        onClick={(a) => setSelectedAgentId(a.id)}
                        matchaBoost={matchaAgentId === 'scout'}
                        themeMode={themeMode}
                      />

                      {/* GUARD */}
                      <ChibiBotDesk
                        agent={row1Agents[1]}
                        isSelected={selectedAgentId === 'guard'}
                        isHighlighted={highlightedPr?.agentsInvolved?.includes('guard')}
                        onClick={(a) => setSelectedAgentId(a.id)}
                        matchaBoost={matchaAgentId === 'guard'}
                        themeMode={themeMode}
                      />

                      {/* VET SQUAD (3 Purple Triplets at Desks, label VET under the middle desk just like screenshot!) */}
                      <ChibiBotDesk
                        agent={row1Agents[2]}
                        labelOverride=""
                        subIndex={0}
                        isSelected={selectedAgentId === 'vet'}
                        isHighlighted={highlightedPr?.agentsInvolved?.includes('vet')}
                        onClick={(a) => setSelectedAgentId(a.id)}
                        matchaBoost={matchaAgentId === 'vet'}
                        themeMode={themeMode}
                      />
                      <ChibiBotDesk
                        agent={row1Agents[2]}
                        labelOverride="VET"
                        subIndex={1}
                        isSelected={selectedAgentId === 'vet'}
                        isHighlighted={highlightedPr?.agentsInvolved?.includes('vet')}
                        onClick={(a) => setSelectedAgentId(a.id)}
                        matchaBoost={matchaAgentId === 'vet'}
                        themeMode={themeMode}
                      />
                      <ChibiBotDesk
                        agent={row1Agents[2]}
                        labelOverride=""
                        subIndex={2}
                        isSelected={selectedAgentId === 'vet'}
                        isHighlighted={highlightedPr?.agentsInvolved?.includes('vet')}
                        onClick={(a) => setSelectedAgentId(a.id)}
                        matchaBoost={matchaAgentId === 'vet'}
                        themeMode={themeMode}
                      />

                      {/* WATCHER */}
                      <ChibiBotDesk
                        agent={row1Agents[3]}
                        isSelected={selectedAgentId === 'watcher'}
                        isHighlighted={highlightedPr?.agentsInvolved?.includes('watcher')}
                        onClick={(a) => setSelectedAgentId(a.id)}
                        matchaBoost={matchaAgentId === 'watcher'}
                        themeMode={themeMode}
                      />
                    </div>
                  </div>

                  {/* ROW 2: HUNTER, REPRO, FIXER, REVIEWER, SUBMIT + Center Plant */}
                  <div>
                    <div className="flex items-center gap-2 mb-2">
                      <span className="text-[10px] font-black uppercase tracking-widest px-2 py-0.5 rounded bg-[#1a1a1a] text-white">
                        ROW 02 // HUNT, REPRO, INKING & PUBLISH LINE
                      </span>
                      <div className="h-0.5 flex-1 bg-[#1a1a1a]/15" />
                    </div>

                    <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-6 gap-2 justify-items-center items-end pt-4">
                      {row2Agents.map((ag) => (
                        <ChibiBotDesk
                          key={ag.id}
                          agent={ag}
                          isSelected={selectedAgentId === ag.id}
                          isHighlighted={highlightedPr?.agentsInvolved?.includes(ag.id)}
                          onClick={(a) => setSelectedAgentId(a.id)}
                          matchaBoost={matchaAgentId === ag.id}
                          themeMode={themeMode}
                        />
                      ))}

                      {/* First Potted Plant positioned between desks and MACHINES panel (matches screenshot!) */}
                      <div className="flex flex-col items-center justify-end pb-2">
                        <MangaPlant label="PLANT-01" />
                      </div>
                    </div>
                  </div>
                </div>

                {/* --- RIGHT SUB-PANEL: "MACHINES" SERVER SHRINE (Matches `MACHINES` / `ME` rack in screenshot) --- */}
                <div className="xl:col-span-4 flex flex-col justify-between">
                  <div 
                    className={`manga-card rounded-xl p-4 relative transition-transform ${
                      themeMode === 'midnight' ? 'bg-zinc-900' : 'bg-[#fffdf7]'
                    }`}
                  >
                    {/* Header */}
                    <div className="flex items-center justify-between border-b-2 border-[#1a1a1a] pb-2 mb-3">
                      <div className="flex items-center gap-1.5">
                        <Server className="w-4 h-4 text-emerald-600" />
                        <span className="font-mono font-black text-xs tracking-widest">MACHINES</span>
                      </div>
                      <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-emerald-200 text-[#1a1a1a] border border-[#1a1a1a] font-bold">
                        ME · ONLINE
                      </span>
                    </div>

                    {/* Server Rack + CI Spirit Illustration */}
                    <div className="flex items-end justify-around gap-3 py-1">
                      {/* Tall Manga Server Monolith ("ME") */}
                      <div className="flex flex-col items-center">
                        <svg width="88" height="152" viewBox="0 0 88 152" className="drop-shadow-[3px_3px_0px_#1a1a1a]">
                          {/* Top Node Indicator */}
                          <rect x="36" y="4" width="16" height="10" rx="2" fill="#52525b" stroke="#1a1a1a" strokeWidth="2" />
                          {/* Main Rack Tower */}
                          <rect x="12" y="14" width="64" height="128" rx="5" fill="#27272a" stroke="#1a1a1a" strokeWidth="2.8" />
                          
                          {/* 6 Clickable Server Blades */}
                          {[0, 1, 2, 3, 4, 5].map((slot) => {
                            const y = 22 + slot * 19;
                            const isCiActive = agents.find((a) => a.id === 'ci')?.status === 'WORKING';
                            const isSelectedSlot = selectedBlade === slot;
                            return (
                              <g 
                                key={slot} 
                                transform={`translate(18, ${y})`}
                                onClick={() => {
                                  setSelectedBlade(slot);
                                  setSelectedAgentId('ci');
                                }}
                                className="cursor-pointer"
                              >
                                <rect 
                                  x="0" 
                                  y="0" 
                                  width="52" 
                                  height="14" 
                                  rx="2" 
                                  fill={isSelectedSlot ? '#3f3f46' : '#18181b'} 
                                  stroke={isSelectedSlot ? '#facc15' : '#1a1a1a'} 
                                  strokeWidth="1.8" 
                                />
                                <line x1="6" y1="7" x2="26" y2="7" stroke="#71717a" strokeWidth="2" strokeDasharray="3 2" />
                                <circle 
                                  cx="36" 
                                  cy="7" 
                                  r="2.2" 
                                  fill={isCiActive ? '#facc15' : '#4ade80'} 
                                />
                                <circle 
                                  cx="44" 
                                  cy="7" 
                                  r="2.2" 
                                  fill={slot % 2 === 0 ? '#f43f5e' : '#38bdf8'} 
                                />
                              </g>
                            );
                          })}
                        </svg>
                        <span className="mt-1.5 font-mono font-black text-xs px-2.5 py-0.5 rounded border-2 border-[#1a1a1a] bg-white text-[#1a1a1a] shadow-[2px_2px_0px_#1a1a1a]">
                          ME
                        </span>
                      </div>

                      {/* Blade Telemetry + Bottom-Right Corner Plant */}
                      <div className="flex flex-col items-center justify-between gap-3">
                        <div 
                          onClick={() => setSelectedAgentId('ci')}
                          className="cursor-pointer p-2.5 rounded-lg border-2 border-[#1a1a1a] bg-emerald-50 text-[#1a1a1a] text-left w-[142px] shadow-[2px_2px_0px_#1a1a1a]"
                        >
                          <div className="flex items-center justify-between text-[10px] font-black text-emerald-800">
                            <span>BLADE #{selectedBlade + 1}</span>
                            <span>PYTEST</span>
                          </div>
                          <div className="text-[11px] font-mono font-bold mt-1 truncate">
                            {agents.find((a) => a.id === 'ci')?.taskText}
                          </div>
                          <div className="mt-2 space-y-1">
                            <div className="w-full bg-zinc-200 h-1.5 rounded-full overflow-hidden border border-[#1a1a1a]">
                              <div className="bg-emerald-500 h-full w-1/3" />
                            </div>
                            <div className="text-[9px] font-mono opacity-75">
                              CPU 14% · VRAM 2.1GB
                            </div>
                          </div>
                        </div>

                        {/* Bottom-right potted plant from screenshot */}
                        <MangaPlant label="PLANT-02" />
                      </div>
                    </div>
                  </div>
                </div>

              </div>
            </div>
          </div>

          {/* -------------------- INTERACTIVE AGENT CHARACTER BIO & MODEL SWITCHER STRIP -------------------- */}
          <div className={`manga-panel rounded-xl p-3.5 ${theme.panelBg} flex flex-col md:flex-row items-start md:items-center justify-between gap-4`}>
            <div className="flex items-start sm:items-center gap-3.5">
              {/* Character Avatar Badge */}
              <div 
                className="w-14 h-14 rounded-xl border-2 border-[#1a1a1a] flex flex-col items-center justify-center shrink-0 shadow-[3px_3px_0px_#1a1a1a]"
                style={{ backgroundColor: selectedAgent.color }}
              >
                <span className="text-[10px] font-black text-[#1a1a1a] leading-none">{selectedAgent.jpName}</span>
                <span className="font-black text-xs text-[#1a1a1a] mt-0.5">{selectedAgent.name}</span>
              </div>

              {/* Bio Details + Prompt Preview */}
              <div className="space-y-1">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="font-black text-sm sm:text-base tracking-tight">
                    {selectedAgent.name} — {selectedAgent.role}
                  </h3>
                  <span className="text-[11px] font-mono font-bold px-2 py-0.5 rounded border border-[#1a1a1a] bg-pink-100 text-[#1a1a1a]">
                    Tokens: {selectedAgent.tokens}k tok
                  </span>
                </div>
                <p className="text-xs opacity-80">
                  {selectedAgent.specialty}
                </p>
                <div className="flex flex-wrap items-center gap-2 pt-0.5">
                  <span className="bg-white text-[#1a1a1a] border border-[#1a1a1a] px-2 py-0.5 rounded text-xs font-bold">
                    💬 &ldquo;{selectedAgent.speech}&rdquo;
                  </span>
                  <code className="text-[10px] font-mono bg-zinc-900 text-emerald-300 px-2 py-0.5 rounded border border-[#1a1a1a]">
                    $ {selectedAgent.promptSnippet}
                  </code>
                </div>
              </div>
            </div>

            {/* Right Controls: Model Switcher + Ping Agent + Matcha Boost */}
            <div className="flex flex-wrap items-center gap-2 shrink-0 self-end md:self-center">
              {/* Model Selector */}
              <div className="flex items-center gap-1 bg-white text-[#1a1a1a] border-2 border-[#1a1a1a] rounded-lg px-2 py-1 shadow-[2px_2px_0px_#1a1a1a]">
                <Sliders className="w-3.5 h-3.5 text-purple-600" />
                <span className="text-[10px] font-black uppercase">Model:</span>
                <select
                  value={selectedAgent.model}
                  onChange={(e) => handleChangeAgentModel(selectedAgent.id, e.target.value)}
                  className="text-xs font-mono font-black bg-transparent focus:outline-none cursor-pointer"
                >
                  <option value="Haiku">Haiku</option>
                  <option value="Sonnet">Sonnet</option>
                  <option value="Opus">Opus</option>
                  <option value="no model">no model</option>
                </select>
              </div>

              <button
                onClick={() => handleSoloAgentAction(selectedAgent)}
                className="manga-btn px-3 py-1.5 rounded-lg bg-amber-200 text-[#1a1a1a] font-black text-xs flex items-center gap-1"
              >
                <Play className="w-3.5 h-3.5 fill-current" /> Test Agent
              </button>

              <button
                onClick={() => handleGiveMatcha(selectedAgent.id)}
                className="manga-btn px-3 py-1.5 rounded-lg bg-emerald-300 text-[#1a1a1a] font-black text-xs flex items-center gap-1.5"
              >
                <Coffee className="w-3.5 h-3.5" /> Matcha 🍵
              </button>
            </div>
          </div>
        </div>

        {/* -------------------- RIGHT 4 COLS: 3 STACKED MANGA STORYBOARD PANELS (QUEUE, PRs, LOG) -------------------- */}
        <div className="lg:col-span-4 flex flex-col gap-4">
          
          {/* PANEL 1: QUEUE */}
          <div className={`manga-panel rounded-xl p-4 ${theme.panelBg}`}>
            <div className="flex items-center justify-between border-b-2 border-[#1a1a1a] pb-2 mb-3">
              <div className="flex items-center gap-2">
                <span className="px-2 py-0.5 bg-[#1a1a1a] text-white font-black text-xs rounded">
                  QUEUE
                </span>
                <span className="text-[11px] font-mono opacity-60">キュー</span>
              </div>
              <span className="text-xs font-mono font-bold">
                {activeWaitTask ? '1 waiting' : 'nothing waiting'}
              </span>
            </div>

            {activeWaitTask ? (
              <div className="mb-3 p-2.5 rounded-lg border-2 border-[#1a1a1a] bg-yellow-200 text-[#1a1a1a] shadow-[2px_2px_0px_#1a1a1a] animate-pulse">
                <div className="flex items-center justify-between text-xs font-black">
                  <span className="bg-rose-500 text-white px-1.5 py-0.5 rounded text-[10px]">IN PROGRESS</span>
                  <span className="font-mono">{activeWaitTask.id}</span>
                </div>
                <div className="font-bold text-xs mt-1">{activeWaitTask.title}</div>
              </div>
            ) : (
              <div className="text-xs font-mono opacity-60 mb-3">
                nothing waiting
              </div>
            )}

            <div className="text-[10px] font-mono font-black uppercase tracking-wider opacity-60 mb-2">
              RECENT
            </div>

            <div className="flex flex-col gap-2.5 max-h-[175px] overflow-y-auto pr-1">
              {queue.map((item) => (
                <div
                  key={item.id}
                  className={`p-2.5 rounded-lg border-2 border-[#1a1a1a] ${theme.cardBg} flex items-start justify-between gap-2 shadow-[2px_2px_0px_#1a1a1a]`}
                >
                  <div className="flex items-start gap-2 min-w-0">
                    <span className="px-2 py-0.5 rounded border-2 border-[#1a1a1a] bg-emerald-300 text-[#1a1a1a] font-mono font-black text-[10px] shrink-0 shadow-[1px_1px_0px_#1a1a1a]">
                      DONE
                    </span>
                    <div className="min-w-0">
                      <div className="font-bold text-xs truncate">{item.title}</div>
                      <div className="text-[10px] font-mono opacity-60 truncate">
                        {item.url}
                      </div>
                    </div>
                  </div>
                  <span className="text-[11px] font-mono opacity-75 shrink-0">
                    {item.id}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* PANEL 2: PULL REQUESTS */}
          <div className={`manga-panel rounded-xl p-4 ${theme.panelBg}`}>
            <div className="flex items-center justify-between border-b-2 border-[#1a1a1a] pb-2 mb-3">
              <div className="flex items-center gap-2">
                <span className="px-2 py-0.5 bg-[#1a1a1a] text-white font-black text-xs rounded">
                  PULL REQUESTS
                </span>
                <span className="text-xs font-mono font-bold opacity-75">
                  {openPrCount} OPEN · {mergedPrCount} MERGED
                </span>
              </div>
              {highlightedPrId && (
                <button
                  onClick={() => setHighlightedPrId(null)}
                  className="text-[10px] font-bold underline text-pink-600"
                >
                  Clear filter
                </button>
              )}
            </div>

            <div className="flex flex-col gap-2.5 max-h-[195px] overflow-y-auto pr-1">
              {prs.map((pr) => {
                const isSelectedPr = highlightedPrId === pr.id;
                const isMerged = pr.status === 'MERGED';
                return (
                  <div
                    key={pr.id}
                    onClick={() => setHighlightedPrId(isSelectedPr ? null : pr.id)}
                    className={`p-2.5 rounded-lg border-2 border-[#1a1a1a] cursor-pointer transition-all flex flex-col gap-2 shadow-[2px_2px_0px_#1a1a1a] ${
                      isSelectedPr
                        ? 'bg-pink-100 text-[#1a1a1a]'
                        : `${theme.cardBg} hover:bg-sky-50/60`
                    }`}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2 min-w-0">
                        <span
                          className={`px-2 py-0.5 rounded border-2 border-[#1a1a1a] font-mono font-black text-[10px] shrink-0 ${
                            isMerged
                              ? 'bg-purple-300 text-[#1a1a1a]'
                              : 'bg-sky-300 text-[#1a1a1a]'
                          }`}
                        >
                          {pr.status}
                        </span>
                        <span className="font-bold text-xs truncate">
                          {pr.title}
                        </span>
                      </div>
                      <span className="text-[11px] font-mono opacity-75 shrink-0">
                        {pr.id}
                      </span>
                    </div>

                    {/* Interactive PR Actions: Inspect Diff Manuscript or Merge */}
                    <div className="flex items-center justify-between pt-1 border-t border-dashed border-zinc-300/70 text-[10px]">
                      <span className="font-mono opacity-65 truncate">{pr.file}</span>
                      <div className="flex items-center gap-1.5">
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setInspectedPr(pr);
                          }}
                          className="px-2 py-0.5 rounded border border-[#1a1a1a] bg-amber-100 text-[#1a1a1a] font-bold flex items-center gap-1 hover:bg-amber-200"
                        >
                          <FileCode2 className="w-3 h-3" /> Diff
                        </button>
                        {!isMerged && (
                          <button
                            onClick={(e) => handleMergePr(pr.id, e)}
                            className="px-2 py-0.5 rounded border border-[#1a1a1a] bg-emerald-300 text-[#1a1a1a] font-black flex items-center gap-1 hover:bg-emerald-400"
                          >
                            <GitMerge className="w-3 h-3" /> Merge
                          </button>
                        )}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>

          {/* PANEL 3: DIALOGUE & ACTION LOG */}
          <div className={`manga-panel rounded-xl p-4 ${theme.panelBg}`}>
            <div className="flex items-center justify-between border-b-2 border-[#1a1a1a] pb-2 mb-2.5">
              <div className="flex items-center gap-2">
                <span className="px-2 py-0.5 bg-[#1a1a1a] text-white font-black text-xs rounded">
                  LOG
                </span>
                <span className="text-[11px] font-mono opacity-60">マイ・オフィス記録</span>
              </div>
              {/* Filter Input */}
              <div className="relative">
                <input
                  type="text"
                  value={logSearch}
                  onChange={(e) => setLogSearch(e.target.value)}
                  placeholder="Filter logs..."
                  className="px-2 py-0.5 text-[10px] font-mono rounded border border-[#1a1a1a] bg-white text-[#1a1a1a] w-28 focus:outline-none focus:ring-1 focus:ring-pink-400"
                />
              </div>
            </div>

            <div className="font-mono text-[11px] space-y-1.5 max-h-[210px] overflow-y-auto pr-1">
              {filteredLogs.map((entry, idx) => (
                <div
                  key={idx}
                  className="flex items-start gap-2 py-0.5 border-b border-dashed border-zinc-300/60 last:border-none"
                >
                  <span className="opacity-50 shrink-0">{entry.time}</span>
                  <span
                    className="font-black shrink-0 w-16 truncate"
                    style={{ color: entry.color }}
                  >
                    {entry.agent}
                  </span>
                  <span className="opacity-90 break-all">{entry.text}</span>
                </div>
              ))}
            </div>
          </div>

        </div>
      </main>

      {/* ==================== MODAL 1: MANGA GIT DIFF MANUSCRIPT VIEWER ==================== */}
      {inspectedPr && (
        <div className="fixed inset-0 z-50 bg-black/50 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="manga-panel rounded-2xl bg-[#fffdf9] text-[#1a1a1a] max-w-lg w-full p-5 relative">
            <button
              onClick={() => setInspectedPr(null)}
              className="absolute top-3 right-3 w-7 h-7 rounded-full border-2 border-[#1a1a1a] bg-rose-300 flex items-center justify-center font-black"
            >
              <X className="w-4 h-4" />
            </button>

            <div className="flex items-center gap-2 mb-2">
              <span className="px-2 py-0.5 bg-[#1a1a1a] text-white font-black text-xs rounded">
                MY OFFICE PATCH MANUSCRIPT
              </span>
              <span className="font-mono text-xs font-bold">{inspectedPr.id}</span>
              <span className="px-2 py-0.5 rounded border border-[#1a1a1a] bg-sky-200 text-[10px] font-black">
                {inspectedPr.status}
              </span>
            </div>

            <h3 className="font-black text-base mb-1">{inspectedPr.title}</h3>
            <p className="text-xs opacity-80 mb-3">{inspectedPr.summary}</p>

            <div className="rounded-xl border-2 border-[#1a1a1a] bg-[#18181b] text-zinc-100 p-3 font-mono text-xs mb-4 shadow-[3px_3px_0px_#1a1a1a]">
              <div className="text-[10px] text-zinc-400 border-b border-zinc-700 pb-1 mb-2">
                --- a/{inspectedPr.file} <br />
                +++ b/{inspectedPr.file}
              </div>
              <pre className="text-rose-400 bg-rose-950/40 px-2 py-1 rounded overflow-x-auto">
                - {inspectedPr.diffRemoved}
              </pre>
              <pre className="text-emerald-400 bg-emerald-950/40 px-2 py-1 rounded mt-1 overflow-x-auto">
                + {inspectedPr.diffAdded}
              </pre>
            </div>

            <div className="flex items-center justify-between">
              <div className="text-[11px] font-bold">
                Verified by: <span className="font-mono uppercase">{inspectedPr.agentsInvolved.join(' → ')}</span>
              </div>
              {inspectedPr.status !== 'MERGED' && (
                <button
                  onClick={() => handleMergePr(inspectedPr.id)}
                  className="manga-btn px-4 py-1.5 rounded-lg bg-emerald-300 text-[#1a1a1a] text-xs font-black flex items-center gap-1.5"
                >
                  <GitMerge className="w-4 h-4" /> Merge PR Now
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      {/* ==================== MODAL 2: REPOS / MACHINES QUICK INSPECTOR ==================== */}
      {activeHeaderModal && (
        <div className="fixed inset-0 z-50 bg-black/50 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="manga-panel rounded-2xl bg-[#fffdf9] text-[#1a1a1a] max-w-md w-full p-5 relative">
            <button
              onClick={() => setActiveHeaderModal(null)}
              className="absolute top-3 right-3 w-7 h-7 rounded-full border-2 border-[#1a1a1a] bg-rose-300 flex items-center justify-center font-black"
            >
              <X className="w-4 h-4" />
            </button>

            {activeHeaderModal === 'repos' ? (
              <div>
                <div className="flex items-center gap-2 mb-3">
                  <span className="px-2 py-0.5 bg-[#1a1a1a] text-white font-black text-xs rounded">
                    MY OFFICE REPOSITORIES
                  </span>
                </div>
                <div className="space-y-2.5 text-xs">
                  <div className="p-3 rounded-xl border-2 border-[#1a1a1a] bg-white shadow-[2px_2px_0px_#1a1a1a]">
                    <div className="flex items-center justify-between font-black">
                      <span>Silvertree2010/ao-testbed</span>
                      <span className="px-1.5 py-0.5 bg-emerald-200 border border-[#1a1a1a] rounded text-[10px]">ACTIVE</span>
                    </div>
                    <div className="font-mono text-[11px] opacity-75 mt-1">
                      Branch: main · Pytest suite: 42 passing · Auto-PR: Enabled
                    </div>
                  </div>
                </div>
              </div>
            ) : (
              <div>
                <div className="flex items-center gap-2 mb-3">
                  <span className="px-2 py-0.5 bg-[#1a1a1a] text-white font-black text-xs rounded">
                    MY OFFICE MACHINES (ME RACK)
                  </span>
                </div>
                <div className="p-3 rounded-xl border-2 border-[#1a1a1a] bg-white shadow-[2px_2px_0px_#1a1a1a] text-xs space-y-2">
                  <div className="flex items-center justify-between font-black">
                    <span>Node: ME (Local Workstation)</span>
                    <span className="px-1.5 py-0.5 bg-emerald-300 border border-[#1a1a1a] rounded text-[10px]">6 BLADES READY</span>
                  </div>
                  <div className="font-mono text-[11px] opacity-80">
                    OS: Linux x86_64 · Sandbox: isolated venv · Token Throughput: 558k tok total
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {/* ==================== MODAL 3: DISPATCH NEW CUSTOM ISSUE ==================== */}
      {showNewBugModal && (
        <div className="fixed inset-0 z-50 bg-black/50 backdrop-blur-xs flex items-center justify-center p-4">
          <div className="manga-panel rounded-2xl bg-[#fffdf9] text-[#1a1a1a] max-w-md w-full p-5 relative">
            <button
              onClick={() => setShowNewBugModal(false)}
              className="absolute top-3 right-3 w-7 h-7 rounded-full border-2 border-[#1a1a1a] bg-rose-300 flex items-center justify-center font-black"
            >
              <X className="w-4 h-4" />
            </button>

            <div className="flex items-center gap-2 mb-3">
              <span className="px-2 py-0.5 bg-[#1a1a1a] text-white font-black text-xs rounded">
                MY OFFICE // NEW ISSUE
              </span>
              <span className="text-xs font-bold text-pink-600">新エピソード</span>
            </div>

            <p className="text-xs opacity-80 mb-3">
              Enter a bug description below and watch your 10 chibi manga agents in <strong>MY OFFICE</strong> triage, reproduce, patch, test, and open a PR for it!
            </p>

            <input
              type="text"
              value={customBugTitle}
              onChange={(e) => setCustomBugTitle(e.target.value)}
              placeholder="e.g., binary_search() overflows on high mid index"
              className="w-full px-3 py-2 rounded-lg border-2 border-[#1a1a1a] bg-white font-mono text-xs mb-4 focus:outline-none focus:ring-2 focus:ring-pink-400"
            />

            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowNewBugModal(false)}
                className="manga-btn px-3 py-1.5 rounded-lg bg-zinc-200 text-xs font-black"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  const title = customBugTitle.trim() || 'custom_hook() fails on empty array input';
                  setCustomBugTitle('');
                  setShowNewBugModal(false);
                  startBugHuntSimulation(title);
                }}
                className="manga-btn px-4 py-1.5 rounded-lg bg-pink-400 text-[#1a1a1a] text-xs font-black flex items-center gap-1"
              >
                <Send className="w-3.5 h-3.5" /> Dispatch to MY OFFICE!
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}