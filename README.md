<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Chameleon — Terminal Emulator</title>
<style>
  @import url('https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@300;400;500;700;800&family=Space+Grotesk:wght@300;400;500;600;700&display=swap');

:root {
--bg: #0a0e14;
--bg2: #0f1520;
--bg3: #141c2b;
--surface: #1a2235;
--border: #1f2d45;
--text: #c8d8f0;
--muted: #5a7090;
--accent1: #00e5a0;
--accent2: #00aaff;
--accent3: #ff6b6b;
--accent4: #ffd93d;
--accent5: #c084fc;
--glow1: rgba(0,229,160,0.15);
--glow2: rgba(0,170,255,0.15);
}

- { margin: 0; padding: 0; box-sizing: border-box; }

html { scroll-behavior: smooth; }

body {
background: var(--bg);
color: var(--text);
font-family: 'Space Grotesk', sans-serif;
font-size: 16px;
line-height: 1.7;
overflow-x: hidden;
}

/_ ─── NOISE OVERLAY ─── _/
body::before {
content: '';
position: fixed;
inset: 0;
background-image: url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)' opacity='0.03'/%3E%3C/svg%3E");
pointer-events: none;
z-index: 9999;
opacity: 0.4;
}

/_ ─── SCANLINES ─── _/
body::after {
content: '';
position: fixed;
inset: 0;
background: repeating-linear-gradient(0deg, transparent, transparent 2px, rgba(0,0,0,0.03) 2px, rgba(0,0,0,0.03) 4px);
pointer-events: none;
z-index: 9998;
}

/_ ─── HERO ─── _/
.hero {
min-height: 100vh;
display: flex;
flex-direction: column;
align-items: center;
justify-content: center;
text-align: center;
padding: 4rem 2rem;
position: relative;
overflow: hidden;
}

.hero-bg {
position: absolute;
inset: 0;
background:
radial-gradient(ellipse 60% 40% at 20% 30%, rgba(0,229,160,0.06) 0%, transparent 60%),
radial-gradient(ellipse 50% 35% at 80% 70%, rgba(0,170,255,0.06) 0%, transparent 60%),
radial-gradient(ellipse 40% 30% at 50% 50%, rgba(192,132,252,0.04) 0%, transparent 60%);
animation: bgPulse 8s ease-in-out infinite alternate;
}

@keyframes bgPulse {
from { opacity: 0.7; }
to { opacity: 1; }
}

/_ Grid lines _/
.hero-grid {
position: absolute;
inset: 0;
background-image:
linear-gradient(rgba(0,229,160,0.04) 1px, transparent 1px),
linear-gradient(90deg, rgba(0,229,160,0.04) 1px, transparent 1px);
background-size: 60px 60px;
mask-image: radial-gradient(ellipse 80% 80% at 50% 50%, black 40%, transparent 100%);
animation: gridDrift 20s linear infinite;
}

@keyframes gridDrift {
from { transform: translateY(0); }
to { transform: translateY(60px); }
}

/_ ─── LOGO ─── _/
.logo-wrap {
position: relative;
margin-bottom: 2rem;
animation: fadeDown 0.8s ease both;
}

.logo-ascii {
font-family: 'JetBrains Mono', monospace;
font-size: clamp(0.55rem, 1.4vw, 0.85rem);
line-height: 1.15;
font-weight: 700;
letter-spacing: 0.05em;
background: linear-gradient(135deg, var(--accent1) 0%, var(--accent2) 40%, var(--accent5) 100%);
-webkit-background-clip: text;
-webkit-text-fill-color: transparent;
background-clip: text;
filter: drop-shadow(0 0 20px rgba(0,229,160,0.4));
white-space: pre;
animation: logoGlow 3s ease-in-out infinite alternate;
}

@keyframes logoGlow {
from { filter: drop-shadow(0 0 12px rgba(0,229,160,0.3)); }
to { filter: drop-shadow(0 0 28px rgba(0,170,255,0.5)) drop-shadow(0 0 8px rgba(192,132,252,0.3)); }
}

.logo-ring {
position: absolute;
inset: -20px;
border-radius: 50%;
border: 1px solid rgba(0,229,160,0.1);
animation: ringPulse 4s ease-in-out infinite;
}

@keyframes ringPulse {
0%, 100% { transform: scale(1); opacity: 0.4; }
50% { transform: scale(1.05); opacity: 0.8; }
}

/_ ─── BADGE ROW ─── _/
.badges {
display: flex;
gap: 0.6rem;
flex-wrap: wrap;
justify-content: center;
margin-bottom: 2rem;
animation: fadeUp 0.8s 0.2s ease both;
}

.badge {
font-family: 'JetBrains Mono', monospace;
font-size: 0.7rem;
font-weight: 700;
padding: 0.25rem 0.75rem;
border-radius: 999px;
border: 1px solid;
letter-spacing: 0.08em;
text-transform: uppercase;
}

.badge-green { color: var(--accent1); border-color: rgba(0,229,160,0.4); background: rgba(0,229,160,0.07); }
.badge-blue { color: var(--accent2); border-color: rgba(0,170,255,0.4); background: rgba(0,170,255,0.07); }
.badge-purple { color: var(--accent5); border-color: rgba(192,132,252,0.4); background: rgba(192,132,252,0.07); }
.badge-yellow { color: var(--accent4); border-color: rgba(255,217,61,0.4); background: rgba(255,217,61,0.07); }
.badge-red { color: var(--accent3); border-color: rgba(255,107,107,0.4); background: rgba(255,107,107,0.07); }

/_ ─── TAGLINE ─── _/
.tagline {
font-size: clamp(1.4rem, 3.5vw, 2.2rem);
font-weight: 700;
letter-spacing: -0.02em;
margin-bottom: 1rem;
animation: fadeDown 0.8s 0.1s ease both;
}

.tagline .hi { color: var(--accent1); }
.tagline .mid { color: var(--text); }
.tagline .lo { color: var(--accent2); }

.subtitle {
font-size: 1.05rem;
color: var(--muted);
max-width: 560px;
margin: 0 auto 2.5rem;
animation: fadeUp 0.8s 0.3s ease both;
font-weight: 400;
}

/_ ─── CTA BUTTONS ─── _/
.cta-row {
display: flex; gap: 1rem; flex-wrap: wrap; justify-content: center;
animation: fadeUp 0.8s 0.4s ease both;
margin-bottom: 3rem;
}

.btn {
font-family: 'JetBrains Mono', monospace;
font-size: 0.82rem;
font-weight: 700;
padding: 0.7rem 1.6rem;
border-radius: 6px;
border: none;
cursor: pointer;
text-decoration: none;
display: inline-flex;
align-items: center;
gap: 0.5rem;
letter-spacing: 0.04em;
transition: transform 0.18s, box-shadow 0.18s, filter 0.18s;
position: relative;
overflow: hidden;
}

.btn::before {
content: '';
position: absolute;
inset: 0;
background: linear-gradient(135deg, rgba(255,255,255,0.1), transparent);
opacity: 0;
transition: opacity 0.2s;
}

.btn:hover::before { opacity: 1; }
.btn:hover { transform: translateY(-2px); }

.btn-primary {
background: linear-gradient(135deg, var(--accent1), #00c890);
color: #001a10;
box-shadow: 0 4px 20px rgba(0,229,160,0.35);
}
.btn-primary:hover { box-shadow: 0 6px 28px rgba(0,229,160,0.55); }

.btn-secondary {
background: var(--surface);
color: var(--text);
border: 1px solid var(--border);
box-shadow: 0 2px 12px rgba(0,0,0,0.3);
}
.btn-secondary:hover { border-color: var(--accent2); box-shadow: 0 4px 18px rgba(0,170,255,0.2); }

/_ ─── TERMINAL WINDOW MOCK ─── _/
.term-demo {
width: min(700px, 90vw);
border-radius: 12px;
border: 1px solid var(--border);
overflow: hidden;
background: var(--bg2);
box-shadow: 0 20px 60px rgba(0,0,0,0.6), 0 0 0 1px rgba(255,255,255,0.04);
animation: fadeUp 0.8s 0.5s ease both;
margin: 0 auto;
}

.term-titlebar {
background: var(--bg3);
padding: 0.65rem 1rem;
display: flex;
align-items: center;
gap: 0.55rem;
border-bottom: 1px solid var(--border);
}

.dot { width: 12px; height: 12px; border-radius: 50%; }
.dot-r { background: #ff5f57; }
.dot-y { background: #febc2e; }
.dot-g { background: #28c840; }

.term-title {
font-family: 'JetBrains Mono', monospace;
font-size: 0.7rem;
color: var(--muted);
margin-left: auto;
margin-right: auto;
}

.term-body {
padding: 1.2rem 1.4rem;
font-family: 'JetBrains Mono', monospace;
font-size: 0.82rem;
line-height: 1.8;
min-height: 200px;
}

.term-line { display: flex; gap: 0.6rem; align-items: flex-start; }
.term-ps1 { color: var(--accent1); white-space: nowrap; }
.term-path { color: var(--accent2); }
.term-cmd { color: var(--text); }
.term-out { color: var(--muted); padding-left: 1.2rem; }
.term-ai-bar {
background: rgba(0,229,160,0.07);
border: 1px solid rgba(0,229,160,0.25);
border-radius: 6px;
padding: 0.5rem 1rem;
margin: 0.4rem 0;
color: var(--accent1);
display: flex;
align-items: center;
gap: 0.5rem;
}
.term-ai-res {
color: var(--accent4);
padding-left: 1.2rem;
animation: typeIn 1.2s 2.5s steps(40) both;
white-space: pre;
overflow: hidden;
max-width: 100%;
}

.cursor {
display: inline-block;
width: 8px; height: 1.1em;
background: var(--accent1);
vertical-align: text-bottom;
animation: blink 1s step-end infinite;
border-radius: 1px;
}

@keyframes blink { 0%, 100% { opacity: 1; } 50% { opacity: 0; } }

@keyframes typeIn {
from { max-width: 0; }
to { max-width: 100%; }
}

/_ ─── SECTION WRAPPER ─── _/
.section {
max-width: 900px;
margin: 0 auto;
padding: 4rem 2rem;
}

.section-label {
font-family: 'JetBrains Mono', monospace;
font-size: 0.68rem;
font-weight: 700;
letter-spacing: 0.2em;
text-transform: uppercase;
color: var(--accent1);
margin-bottom: 0.6rem;
display: flex;
align-items: center;
gap: 0.6rem;
}

.section-label::after {
content: '';
flex: 1;
height: 1px;
background: linear-gradient(90deg, rgba(0,229,160,0.3), transparent);
}

.section-title {
font-size: clamp(1.5rem, 3vw, 2rem);
font-weight: 700;
letter-spacing: -0.02em;
margin-bottom: 2rem;
}

/_ ─── FEATURE GRID ─── _/
.feature-grid {
display: grid;
grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
gap: 1px;
background: var(--border);
border: 1px solid var(--border);
border-radius: 12px;
overflow: hidden;
}

.feature-card {
background: var(--bg2);
padding: 1.6rem;
transition: background 0.2s;
cursor: default;
}

.feature-card:hover { background: var(--surface); }

.feature-icon {
font-size: 1.6rem;
margin-bottom: 0.8rem;
display: block;
}

.feature-name {
font-weight: 700;
font-size: 0.95rem;
margin-bottom: 0.4rem;
color: var(--text);
}

.feature-desc { font-size: 0.85rem; color: var(--muted); line-height: 1.6; }

/_ ─── KEYBINDS ─── _/
.kb-grid {
display: grid;
grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
gap: 0.5rem;
}

.kb-row {
display: flex;
align-items: center;
gap: 1rem;
padding: 0.7rem 1rem;
background: var(--bg2);
border: 1px solid var(--border);
border-radius: 8px;
transition: border-color 0.2s, background 0.2s;
}

.kb-row:hover { border-color: rgba(0,170,255,0.3); background: var(--surface); }

.kb-keys {
display: flex;
gap: 0.3rem;
flex-shrink: 0;
}

.key {
font-family: 'JetBrains Mono', monospace;
font-size: 0.65rem;
font-weight: 700;
padding: 0.2rem 0.45rem;
background: var(--bg3);
border: 1px solid var(--border);
border-bottom-width: 2px;
border-radius: 4px;
color: var(--accent2);
white-space: nowrap;
box-shadow: 0 1px 0 rgba(0,0,0,0.5);
}

.kb-action { font-size: 0.85rem; color: var(--muted); }

/_ ─── CONFIG ─── _/
.config-block {
background: var(--bg2);
border: 1px solid var(--border);
border-radius: 10px;
overflow: hidden;
}

.config-header {
background: var(--bg3);
padding: 0.6rem 1.2rem;
font-family: 'JetBrains Mono', monospace;
font-size: 0.72rem;
color: var(--muted);
border-bottom: 1px solid var(--border);
display: flex;
align-items: center;
gap: 0.6rem;
}

.config-header span { color: var(--accent1); }

pre {
padding: 1.4rem 1.6rem;
font-family: 'JetBrains Mono', monospace;
font-size: 0.82rem;
line-height: 1.9;
overflow-x: auto;
tab-size: 2;
}

.c-comment { color: #3a5070; }
.c-section { color: var(--accent5); font-weight: 700; }
.c-key { color: var(--accent2); }
.c-eq { color: var(--muted); }
.c-str { color: var(--accent4); }
.c-num { color: var(--accent3); }

/_ ─── ARCH / DEPS ─── _/
.dep-table {
width: 100%;
border-collapse: collapse;
font-size: 0.85rem;
}

.dep-table th {
text-align: left;
padding: 0.7rem 1rem;
font-family: 'JetBrains Mono', monospace;
font-size: 0.7rem;
font-weight: 700;
letter-spacing: 0.1em;
text-transform: uppercase;
color: var(--muted);
border-bottom: 1px solid var(--border);
}

.dep-table td {
padding: 0.75rem 1rem;
border-bottom: 1px solid rgba(31,45,69,0.5);
vertical-align: top;
}

.dep-table tr:hover td { background: rgba(26,34,53,0.5); }

.dep-table tr:last-child td { border-bottom: none; }

.crate-name {
font-family: 'JetBrains Mono', monospace;
font-size: 0.78rem;
color: var(--accent1);
font-weight: 700;
}

/_ ─── INSTALL ─── _/
.install-tabs {
display: flex;
gap: 0.3rem;
margin-bottom: 1rem;
}

.tab-btn {
font-family: 'JetBrains Mono', monospace;
font-size: 0.72rem;
font-weight: 700;
padding: 0.4rem 0.9rem;
background: var(--bg2);
border: 1px solid var(--border);
border-radius: 6px 6px 0 0;
color: var(--muted);
cursor: pointer;
transition: color 0.15s, border-color 0.15s;
border-bottom: none;
}

.tab-btn.active, .tab-btn:hover {
color: var(--accent1);
border-color: rgba(0,229,160,0.4);
background: var(--bg3);
}

.tab-content { display: none; }
.tab-content.active { display: block; }

/_ ─── FOOTER ─── _/
footer {
border-top: 1px solid var(--border);
padding: 2rem;
text-align: center;
font-size: 0.8rem;
color: var(--muted);
font-family: 'JetBrains Mono', monospace;
}

footer .hl { color: var(--accent1); }

/_ ─── TOC SIDEBAR ─── _/
.toc {
position: fixed;
left: 1.5rem;
top: 50%;
transform: translateY(-50%);
display: flex;
flex-direction: column;
gap: 0.5rem;
z-index: 100;
}

@media (max-width: 1100px) { .toc { display: none; } }

.toc-dot {
width: 8px; height: 8px;
border-radius: 50%;
background: var(--border);
cursor: pointer;
transition: background 0.2s, transform 0.2s;
position: relative;
}

.toc-dot:hover, .toc-dot.active {
background: var(--accent1);
transform: scale(1.4);
}

.toc-dot::after {
content: attr(data-label);
position: absolute;
left: 1.4rem;
top: 50%;
transform: translateY(-50%);
background: var(--surface);
border: 1px solid var(--border);
border-radius: 4px;
padding: 0.2rem 0.6rem;
font-size: 0.65rem;
font-family: 'JetBrains Mono', monospace;
color: var(--text);
white-space: nowrap;
opacity: 0;
pointer-events: none;
transition: opacity 0.15s;
}

.toc-dot:hover::after { opacity: 1; }

/_ ─── DIVIDER ─── _/
.divider {
width: 100%;
height: 1px;
background: linear-gradient(90deg, transparent, var(--border), transparent);
}

/_ ─── ANIMATIONS ─── _/
@keyframes fadeDown {
from { opacity: 0; transform: translateY(-20px); }
to { opacity: 1; transform: translateY(0); }
}

@keyframes fadeUp {
from { opacity: 0; transform: translateY(20px); }
to { opacity: 1; transform: translateY(0); }
}

.reveal {
opacity: 0;
transform: translateY(24px);
transition: opacity 0.6s ease, transform 0.6s ease;
}

.reveal.in { opacity: 1; transform: none; }

/_ ─── FLOATING PARTICLES ─── _/
.particles { position: absolute; inset: 0; overflow: hidden; pointer-events: none; }

.particle {
position: absolute;
width: 2px; height: 2px;
border-radius: 50%;
background: var(--accent1);
animation: float linear infinite;
opacity: 0;
}

@keyframes float {
0% { transform: translateY(100vh) translateX(0); opacity: 0; }
5% { opacity: 0.6; }
90% { opacity: 0.3; }
100% { transform: translateY(-10vh) translateX(var(--dx, 30px)); opacity: 0; }
}

/_ color variant particles _/
.particle:nth-child(3n) { background: var(--accent2); }
.particle:nth-child(3n+1) { background: var(--accent5); }

/_ ─── CHAMELEON ANIMATED EYE ─── _/
.eye-wrap {
display: flex;
justify-content: center;
margin: 1rem 0 2rem;
animation: fadeUp 0.8s 0.45s ease both;
}

.cham-svg { width: 120px; height: 60px; }
</style>

</head>
<body>

<!-- Table of contents dots -->
<nav class="toc" id="toc">
  <div class="toc-dot active" data-label="Home"     onclick="scrollTo('#hero')"></div>
  <div class="toc-dot"        data-label="Features" onclick="scrollTo('#features')"></div>
  <div class="toc-dot"        data-label="Keybinds" onclick="scrollTo('#keybinds')"></div>
  <div class="toc-dot"        data-label="Install"  onclick="scrollTo('#install')"></div>
  <div class="toc-dot"        data-label="Config"   onclick="scrollTo('#config')"></div>
  <div class="toc-dot"        data-label="Arch"     onclick="scrollTo('#arch')"></div>
</nav>

<!-- ═══════════════ HERO ═══════════════ -->
<section class="hero" id="hero">
  <div class="hero-bg"></div>
  <div class="hero-grid"></div>

  <!-- floating particles -->
  <div class="particles" id="particles"></div>

  <div class="logo-wrap">
    <div class="logo-ascii" aria-label="Chameleon logo">
 ██████╗██╗  ██╗ █████╗ ███╗   ███╗███████╗██╗     ███████╗ ██████╗ ███╗   ██╗
██╔════╝██║  ██║██╔══██╗████╗ ████║██╔════╝██║     ██╔════╝██╔═══██╗████╗  ██║
██║     ███████║███████║██╔████╔██║█████╗  ██║     █████╗  ██║   ██║██╔██╗ ██║
██║     ██╔══██║██╔══██║██║╚██╔╝██║██╔══╝  ██║     ██╔══╝  ██║   ██║██║╚██╗██║
╚██████╗██║  ██║██║  ██║██║ ╚═╝ ██║███████╗███████╗███████╗╚██████╔╝██║ ╚████║
 ╚═════╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝╚══════╝╚══════╝╚══════╝ ╚═════╝ ╚═╝  ╚═══╝</div>
  </div>

  <!-- Chameleon SVG mascot eye -->
  <div class="eye-wrap">
    <svg class="cham-svg" viewBox="0 0 120 60" fill="none" xmlns="http://www.w3.org/2000/svg">
      <!-- body -->
      <ellipse cx="55" cy="38" rx="38" ry="16" fill="#0f2820" stroke="#00e5a0" stroke-width="1.2"/>
      <!-- tail curl -->
      <path d="M17 38 Q5 44 8 52 Q11 58 18 55" stroke="#00e5a0" stroke-width="1.5" fill="none" stroke-linecap="round"/>
      <!-- legs -->
      <line x1="35" y1="52" x2="30" y2="58" stroke="#00e5a0" stroke-width="1.2" stroke-linecap="round"/>
      <line x1="55" y1="54" x2="52" y2="60" stroke="#00e5a0" stroke-width="1.2" stroke-linecap="round"/>
      <line x1="70" y1="52" x2="68" y2="58" stroke="#00e5a0" stroke-width="1.2" stroke-linecap="round"/>
      <!-- neck -->
      <path d="M93 38 Q100 28 98 20" stroke="#00e5a0" stroke-width="1.5" fill="none" stroke-linecap="round"/>
      <!-- head -->
      <ellipse cx="102" cy="16" rx="14" ry="10" fill="#0f2820" stroke="#00e5a0" stroke-width="1.2"/>
      <!-- eye outer -->
      <circle cx="108" cy="13" r="5.5" fill="#001a10" stroke="#00aaff" stroke-width="1.2"/>
      <!-- eye inner animated -->
      <circle id="pupil" cx="108" cy="13" r="2.5" fill="#00e5a0">
        <animateTransform attributeName="transform" type="translate" values="0 0;1 -0.5;-0.5 1;0.5 0.5;0 0" dur="4s" repeatCount="indefinite"/>
      </circle>
      <!-- eye shine -->
      <circle cx="109.5" cy="11.5" r="1" fill="white" opacity="0.7"/>
      <!-- tongue -->
      <path d="M88 16 Q80 14 76 18" stroke="#ff6b6b" stroke-width="1.4" fill="none" stroke-linecap="round" opacity="0">
        <animate attributeName="opacity" values="0;0;1;1;0" dur="5s" repeatCount="indefinite"/>
        <animate attributeName="d" values="M88 16 Q80 14 76 18;M88 16 Q74 12 68 18;M88 16 Q74 12 68 18;M88 16 Q80 14 76 18;M88 16 Q80 14 76 18" dur="5s" repeatCount="indefinite"/>
      </path>
      <!-- color spots - they shift color like a real chameleon -->
      <circle cx="45" cy="36" r="3" opacity="0.6">
        <animate attributeName="fill" values="#00e5a0;#00aaff;#c084fc;#ffd93d;#00e5a0" dur="6s" repeatCount="indefinite"/>
      </circle>
      <circle cx="60" cy="40" r="2.5" opacity="0.5">
        <animate attributeName="fill" values="#00aaff;#c084fc;#ffd93d;#00e5a0;#00aaff" dur="6s" repeatCount="indefinite"/>
      </circle>
      <circle cx="75" cy="37" r="2" opacity="0.55">
        <animate attributeName="fill" values="#c084fc;#ffd93d;#00e5a0;#00aaff;#c084fc" dur="6s" repeatCount="indefinite"/>
      </circle>
    </svg>
  </div>

  <div class="badges">
    <span class="badge badge-green">🦎 Rust</span>
    <span class="badge badge-blue">⚡ PTY</span>
    <span class="badge badge-purple">🤖 AI Bar</span>
    <span class="badge badge-yellow">🎨 Themeable</span>
    <span class="badge badge-red">🐧 Unix</span>
  </div>

  <h1 class="tagline">
    <span class="hi">Minimal.</span>
    <span class="mid"> Smart.</span>
    <span class="lo"> Alive.</span>
  </h1>

  <p class="subtitle">
    A terminal emulator written in Rust — runs your shell in a PTY, parses escape sequences
    with VTE, renders via crossterm, and brings <strong>AI-powered command suggestions</strong> to your fingertips.
  </p>

  <div class="cta-row">
    <a href="#install" class="btn btn-primary">⬇ Install</a>
    <a href="#features" class="btn btn-secondary">✦ Features</a>
    <a href="#config" class="btn btn-secondary">⚙ Config</a>
  </div>

  <!-- Terminal demo -->
  <div class="term-demo">
    <div class="term-titlebar">
      <span class="dot dot-r"></span>
      <span class="dot dot-y"></span>
      <span class="dot dot-g"></span>
      <span class="term-title">chameleon — zsh — 700×400</span>
    </div>
    <div class="term-body">
      <div class="term-line">
        <span class="term-ps1">❯</span>
        <span class="term-path">~/projects</span>
        <span class="term-cmd">&nbsp;ls -la</span>
      </div>
      <div class="term-out">drwxr-xr-x  9 user staff  288 Mar  5 09:12 chameleon</div>
      <div class="term-out">-rw-r--r--  1 user staff 4.2K Mar  5 09:10 Cargo.toml</div>
      <br>
      <div class="term-line">
        <span class="term-ps1">❯</span>
        <span class="term-path">~/projects</span>
        <span class="term-cmd">&nbsp;<kbd style="background:none;border:none;color:inherit">^K</kbd></span>
      </div>
      <div class="term-ai-bar">
        <span>🤖</span>
        <span>find all rust files modified in last 7 days</span>
      </div>
      <div class="term-out term-ai-res">find . -name "*.rs" -mtime -7 -type f</div>
      <br>
      <div class="term-line">
        <span class="term-ps1">❯</span>
        <span class="term-path">~/projects</span>
        <span>&nbsp;<span class="cursor"></span></span>
      </div>
    </div>
  </div>
</section>

<!-- ═══════════════ FEATURES ═══════════════ -->
<section class="section reveal" id="features">
  <p class="section-label">capabilities</p>
  <h2 class="section-title">What Chameleon can do</h2>

  <div class="feature-grid">
    <div class="feature-card">
      <span class="feature-icon">🔌</span>
      <div class="feature-name">PTY + Shell</div>
      <div class="feature-desc">Spawns your <code>$SHELL</code> (or <code>/bin/sh</code>) in a real pseudo-terminal. Full signal support: SIGINT, SIGTSTP, SIGQUIT, EOF.</div>
    </div>
    <div class="feature-card">
      <span class="feature-icon">🎨</span>
      <div class="feature-name">VTE Escape Parsing</div>
      <div class="feature-desc">Handles cursor movement, 8 standard colors, bold, erase, scroll, and common CSI/ESC sequences via the battle-tested <code>vte</code> crate.</div>
    </div>
    <div class="feature-card">
      <span class="feature-icon">🤖</span>
      <div class="feature-name">AI Command Bar</div>
      <div class="feature-desc">Press <strong>Ctrl+K</strong>. Type plain English. Get a shell command. Supports Ollama (local), OpenAI, Gemini, and Groq.</div>
    </div>
    <div class="feature-card">
      <span class="feature-icon">📐</span>
      <div class="feature-name">Dynamic Resize</div>
      <div class="feature-desc">Window resize propagates to the PTY size and triggers a full redraw. No glitchy stretching or clipping.</div>
    </div>
    <div class="feature-card">
      <span class="feature-icon">📋</span>
      <div class="feature-name">Mouse Copy</div>
      <div class="feature-desc">Click-drag to select. Double-click for word, triple-click for line. Copies to system clipboard on release or via <strong>Ctrl+Shift+C</strong>.</div>
    </div>
    <div class="feature-card">
      <span class="feature-icon">🌈</span>
      <div class="feature-name">Live Theming</div>
      <div class="feature-desc">Edit <code>config.toml</code> and the theme reloads immediately. Press <strong>Ctrl+Shift+T</strong> to open it in <code>$EDITOR</code> without leaving the terminal.</div>
    </div>
  </div>
</section>

<div class="divider"></div>

<!-- ═══════════════ KEYBINDS ═══════════════ -->
<section class="section reveal" id="keybinds">
  <p class="section-label">keyboard</p>
  <h2 class="section-title">Key bindings</h2>

  <div class="kb-grid">
    <div class="kb-row">
      <div class="kb-keys"><span class="key">Ctrl</span><span class="key">K</span></div>
      <div class="kb-action">Open AI command bar</div>
    </div>
    <div class="kb-row">
      <div class="kb-keys"><span class="key">Ctrl</span><span class="key">Shift</span><span class="key">T</span></div>
      <div class="kb-action">Edit config + reload theme</div>
    </div>
    <div class="kb-row">
      <div class="kb-keys"><span class="key">Ctrl</span><span class="key">Shift</span><span class="key">C</span></div>
      <div class="kb-action">Copy selection to clipboard</div>
    </div>
    <div class="kb-row">
      <div class="kb-keys"><span class="key">Ctrl</span><span class="key">C</span></div>
      <div class="kb-action">Send SIGINT to shell</div>
    </div>
    <div class="kb-row">
      <div class="kb-keys"><span class="key">Ctrl</span><span class="key">Z</span></div>
      <div class="kb-action">Send SIGTSTP (suspend)</div>
    </div>
    <div class="kb-row">
      <div class="kb-keys"><span class="key">Ctrl</span><span class="key">D</span></div>
      <div class="kb-action">Send EOF (exit shell)</div>
    </div>
    <div class="kb-row">
      <div class="kb-keys"><span class="key">Ctrl</span><span class="key">\</span></div>
      <div class="kb-action">Send SIGQUIT</div>
    </div>
    <div class="kb-row">
      <div class="kb-keys"><span class="key">Esc</span></div>
      <div class="kb-action">Dismiss AI bar / pickers</div>
    </div>
  </div>

  <br>
  <p style="font-size:0.85rem; color:var(--muted);">
    Standard keys — arrows, Tab, Enter, Backspace, Home, End, Page Up/Down, Delete, Insert — are passed through to the shell unchanged.
  </p>
</section>

<div class="divider"></div>

<!-- ═══════════════ INSTALL ═══════════════ -->
<section class="section reveal" id="install">
  <p class="section-label">getting started</p>
  <h2 class="section-title">Install Chameleon</h2>

  <div class="install-tabs">
    <button class="tab-btn active" onclick="showTab('binary', this)">Prebuilt Binary</button>
    <button class="tab-btn" onclick="showTab('source', this)">From Source</button>
  </div>

  <div id="tab-binary" class="tab-content active">
    <div class="config-block">
      <div class="config-header">
        <span>bash</span> — Linux / macOS
      </div>
      <pre><span style="color:var(--muted)"># Download from Releases, then:</span>
<span style="color:var(--accent1)">tar</span> <span style="color:var(--accent4)">-xzf</span> chameleon-*.tar.gz
<span style="color:var(--accent1)">mv</span> chameleon-*/chameleon ~/bin/

<span style="color:var(--muted)"># Or use /usr/local/bin for system-wide install</span>
<span style="color:var(--accent1)">mv</span> chameleon-\*/chameleon /usr/local/bin/</pre>
</div>
<p style="font-size:0.85rem; color:var(--muted); margin-top:0.8rem;">No Rust or package managers required. Just download and run.</p>

  </div>

  <div id="tab-source" class="tab-content">
    <div class="config-block">
      <div class="config-header">
        <span>bash</span> — Rust edition 2021 required
      </div>
      <pre><span style="color:var(--accent1)">git</span> clone &lt;repo-url&gt;
<span style="color:var(--accent1)">cd</span> chameleon
<span style="color:var(--accent1)">cargo</span> build <span style="color:var(--accent4)">--release</span>

<span style="color:var(--muted)"># Run</span>
<span style="color:var(--accent1)">./target/release/chameleon</span>
<span style="color:var(--muted)"># or</span>
<span style="color:var(--accent1)">cargo</span> run</pre>
</div>

  </div>

  <!-- requirements callout -->
  <div style="display:flex;gap:1rem;flex-wrap:wrap;margin-top:1.5rem;">
    <div style="flex:1;min-width:220px;background:var(--bg2);border:1px solid rgba(0,229,160,0.2);border-radius:8px;padding:1rem;">
      <div style="color:var(--accent1);font-weight:700;font-size:0.85rem;margin-bottom:0.3rem;">🐧 Platform</div>
      <div style="font-size:0.82rem;color:var(--muted);">Linux or macOS (PTY requires Unix-like environment)</div>
    </div>
    <div style="flex:1;min-width:220px;background:var(--bg2);border:1px solid rgba(0,170,255,0.2);border-radius:8px;padding:1rem;">
      <div style="color:var(--accent2);font-weight:700;font-size:0.85rem;margin-bottom:0.3rem;">🤖 AI (optional)</div>
      <div style="font-size:0.82rem;color:var(--muted);">Ollama with ≥1 model, or an API key for OpenAI / Gemini / Groq</div>
    </div>
    <div style="flex:1;min-width:220px;background:var(--bg2);border:1px solid rgba(192,132,252,0.2);border-radius:8px;padding:1rem;">
      <div style="color:var(--accent5);font-weight:700;font-size:0.85rem;margin-bottom:0.3rem;">🦀 Build</div>
      <div style="font-size:0.82rem;color:var(--muted);">Rust edition 2021 (only needed to build from source)</div>
    </div>
  </div>
</section>

<div class="divider"></div>

<!-- ═══════════════ CONFIG ═══════════════ -->
<section class="section reveal" id="config">
  <p class="section-label">configuration</p>
  <h2 class="section-title">config.toml</h2>

  <p style="font-size:0.9rem;color:var(--muted);margin-bottom:1.5rem;">
    Located at <code style="color:var(--accent1)">~/.config/chameleon/config.toml</code>
    (or <code style="color:var(--accent2)">$XDG_CONFIG_HOME/chameleon/config.toml</code>).
    Press <strong>Ctrl+Shift+T</strong> inside Chameleon to open it in your editor and reload on save.
  </p>

  <div class="config-block">
    <div class="config-header">
      <span>~/.config/chameleon/config.toml</span>
    </div>
    <pre><span class="c-comment"># ──────────────────────────────────────
# THEME
# ──────────────────────────────────────</span>
<span class="c-section">[theme]</span>
<span class="c-key">default_foreground</span> <span class="c-eq">=</span> <span class="c-str">"#cccccc"</span>    <span class="c-comment"># text color</span>
<span class="c-key">default_background</span> <span class="c-eq">=</span> <span class="c-str">"#1e1e1e"</span>    <span class="c-comment"># terminal background</span>
<span class="c-key">background_opacity</span> <span class="c-eq">=</span> <span class="c-num">0.95</span>          <span class="c-comment"># 0.0 transparent → 1.0 opaque</span>
<span class="c-key">font_size</span>          <span class="c-eq">=</span> <span class="c-num">14</span>            <span class="c-comment"># points (hint; host may override)</span>

<span class="c-comment"># ──────────────────────────────────────

# AI

# ──────────────────────────────────────</span>

<span class="c-section">[ai]</span>
<span class="c-key">default_backend</span> <span class="c-eq">=</span> <span class="c-str">"ollama"</span> <span class="c-comment"># ollama | openai | gemini | groq</span>
<span class="c-key">base_url</span> <span class="c-eq">=</span> <span class="c-str">"http://127.0.0.1:11434"</span>
<span class="c-key">model</span> <span class="c-eq">=</span> <span class="c-str">"llama3.2:latest"</span>

<span class="c-comment"># API keys (env vars preferred over config file)

# OPENAI_API_KEY / GEMINI_API_KEY / GROQ_API_KEY</span>

<span class="c-section">[ai.providers.openai]</span>
<span class="c-key">api_key</span> <span class="c-eq">=</span> <span class="c-str">"sk-..."</span>

<span class="c-section">[ai.providers.gemini]</span>
<span class="c-key">api_key</span> <span class="c-eq">=</span> <span class="c-str">"..."</span>

<span class="c-section">[ai.providers.groq]</span>
<span class="c-key">api_key</span> <span class="c-eq">=</span> <span class="c-str">"..."</span></pre>

  </div>

  <!-- AI provider tip -->
  <div style="margin-top:1rem;padding:1rem 1.2rem;background:rgba(255,217,61,0.06);border:1px solid rgba(255,217,61,0.25);border-radius:8px;font-size:0.85rem;color:var(--muted);">
    <strong style="color:var(--accent4);">💡 Tip:</strong> Use <strong>/model</strong> or <strong>/models</strong> in the AI bar to switch backends and models live, without editing the config file.
  </div>
</section>

<div class="divider"></div>

<!-- ═══════════════ ARCHITECTURE ═══════════════ -->
<section class="section reveal" id="arch">
  <p class="section-label">internals</p>
  <h2 class="section-title">Architecture &amp; Dependencies</h2>

  <!-- threads diagram -->
  <div style="display:flex;gap:1px;background:var(--border);border:1px solid var(--border);border-radius:10px;overflow:hidden;margin-bottom:2rem;flex-wrap:wrap;">
    <div style="flex:1;min-width:220px;background:var(--bg2);padding:1.4rem;">
      <div style="color:var(--accent1);font-family:'JetBrains Mono',monospace;font-size:0.75rem;font-weight:700;margin-bottom:0.5rem;">MAIN THREAD</div>
      <div style="font-size:0.83rem;color:var(--muted);line-height:1.7;">Crossterm raw mode + alternate screen. Event loop for keyboard &amp; resize. Writes input → PTY master. Redraws from shared buffer when dirty or on timeout.</div>
    </div>
    <div style="flex:1;min-width:220px;background:var(--bg2);padding:1.4rem;border-left:1px solid var(--border);">
      <div style="color:var(--accent2);font-family:'JetBrains Mono',monospace;font-size:0.75rem;font-weight:700;margin-bottom:0.5rem;">READER THREAD</div>
      <div style="font-size:0.83rem;color:var(--muted);line-height:1.7;">Reads bytes from PTY master → feeds into <code>vte::Parser</code> → updates shared screen buffer via <code>Perform</code> implementation → triggers redraw.</div>
    </div>
    <div style="flex:1;min-width:220px;background:var(--bg2);padding:1.4rem;border-left:1px solid var(--border);">
      <div style="color:var(--accent5);font-family:'JetBrains Mono',monospace;font-size:0.75rem;font-weight:700;margin-bottom:0.5rem;">RESIZE</div>
      <div style="font-size:0.83rem;color:var(--muted);line-height:1.7;">PTY size is updated, screen buffer resized, full redraw triggered. Seamless handling of terminal window changes.</div>
    </div>
  </div>

  <!-- deps table -->
  <table class="dep-table">
    <thead>
      <tr>
        <th>Crate</th>
        <th>Role</th>
      </tr>
    </thead>
    <tbody>
      <tr>
        <td><span class="crate-name">crossterm</span></td>
        <td style="color:var(--muted);font-size:0.83rem;">Terminal I/O, raw mode, display, mouse events</td>
      </tr>
      <tr>
        <td><span class="crate-name">portable-pty</span></td>
        <td style="color:var(--muted);font-size:0.83rem;">Cross-platform PTY (pseudo-terminal) support</td>
      </tr>
      <tr>
        <td><span class="crate-name">vte</span></td>
        <td style="color:var(--muted);font-size:0.83rem;">ANSI/VT100 escape sequence parsing</td>
      </tr>
      <tr>
        <td><span class="crate-name">arboard</span></td>
        <td style="color:var(--muted);font-size:0.83rem;">System clipboard integration (copy)</td>
      </tr>
      <tr>
        <td><span class="crate-name">ureq + serde_json</span></td>
        <td style="color:var(--muted);font-size:0.83rem;">HTTP calls to Ollama / OpenAI / Gemini / Groq APIs</td>
      </tr>
      <tr>
        <td><span class="crate-name">serde + toml</span></td>
        <td style="color:var(--muted);font-size:0.83rem;">Config file parsing and serialization</td>
      </tr>
      <tr>
        <td><span class="crate-name">directories</span></td>
        <td style="color:var(--muted);font-size:0.83rem;">XDG-aware config path (<code>~/.config/chameleon</code>)</td>
      </tr>
    </tbody>
  </table>
</section>

<!-- ═══════════════ FOOTER ═══════════════ -->
<footer>
  <div style="margin-bottom:0.5rem;">
    🦎 <span class="hl">Chameleon</span> — A terminal that adapts to you.
  </div>
  <div>Personal & non-commercial use is free. Modification, rebranding, and resale require written permission.</div>
  <div style="margin-top:0.8rem;opacity:0.4;font-size:0.65rem;">Built with ♥ in Rust · PTY + VTE + crossterm</div>
</footer>

<script>
  // ── Particles ──
  const particleContainer = document.getElementById('particles');
  for (let i = 0; i < 28; i++) {
    const p = document.createElement('div');
    p.className = 'particle';
    const left = Math.random() * 100;
    const dur  = 8 + Math.random() * 14;
    const delay = Math.random() * 10;
    const dx = (Math.random() - 0.5) * 80;
    p.style.cssText = `left:${left}%;--dx:${dx}px;animation-duration:${dur}s;animation-delay:-${delay}s;`;
    particleContainer.appendChild(p);
  }

  // ── Scroll reveal ──
  const revealEls = document.querySelectorAll('.reveal');
  const observer = new IntersectionObserver((entries) => {
    entries.forEach(e => { if (e.isIntersecting) { e.target.classList.add('in'); } });
  }, { threshold: 0.12 });
  revealEls.forEach(el => observer.observe(el));

  // ── TOC active dots ──
  const sections = ['hero','features','keybinds','install','config','arch'];
  const dots = document.querySelectorAll('.toc-dot');
  const sectionEls = sections.map(id => document.getElementById(id));

  const tocObserver = new IntersectionObserver((entries) => {
    entries.forEach(e => {
      if (e.isIntersecting) {
        const idx = sectionEls.indexOf(e.target);
        dots.forEach((d,i) => d.classList.toggle('active', i === idx));
      }
    });
  }, { threshold: 0.4 });
  sectionEls.forEach(el => el && tocObserver.observe(el));

  // ── Smooth scroll helper ──
  function scrollTo(selector) {
    document.querySelector(selector)?.scrollIntoView({ behavior: 'smooth' });
  }

  // ── Tabs ──
  function showTab(id, btn) {
    document.querySelectorAll('.tab-content').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    document.getElementById('tab-' + id).classList.add('active');
    btn.classList.add('active');
  }
</script>
</body>
</html>
