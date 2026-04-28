import { defineConfig, presetWind3 } from 'unocss'

export default defineConfig({
  presets: [presetWind3()],
  preflights: [
    {
      getCSS: () => `
*, *::before, *::after { box-sizing: border-box; border: 0 solid transparent; margin: 0; padding: 0; }
html { min-height: 100%; height: 100%; overflow: hidden; -webkit-text-size-adjust: 100%; text-size-adjust: 100%; }
body { min-height: 100vh; min-height: 100dvh; height: 100%; overflow: hidden; line-height: var(--leading-normal); -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; }
img, svg, video { display: block; max-width: 100%; }
input, button, textarea, select { appearance: none; -webkit-appearance: none; border-radius: 0; background: transparent; font: inherit; color: inherit; }
button { cursor: pointer; }
button:disabled { cursor: not-allowed; }
ul, ol { list-style: none; }
a { color: inherit; text-decoration: none; }

:root {
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --space-6: 24px;
  --space-8: 32px;
  --space-10: 40px;
  --font-sans: ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --font-mono: "SF Mono", "JetBrains Mono", ui-monospace, "Cascadia Code", monospace;
  --text-xs: 0.75rem;
  --text-sm: 0.8125rem;
  --text-base: 0.875rem;
  --text-lg: 1rem;
  --text-xl: 1.125rem;
  --leading-tight: 1.25;
  --leading-normal: 1.5;
  --radius-sm: 6px;
  --radius-md: 8px;
  --radius-lg: 12px;
  --radius-xl: 16px;
  --radius-module: 18px;
  --radius-full: 9999px;
  --duration-fast: 120ms;
  --duration-normal: 200ms;
  --duration-slow: 320ms;
  --ease-out: cubic-bezier(0.16, 1, 0.3, 1);
  font-family: var(--font-sans);
  font-size: 16px;
  background: var(--color-bg);
  color: var(--color-text);
}

[data-theme='light'] {
  color-scheme: light;
  --color-bg: #f7f9ff;
  --color-bg-subtle: #eff4ff;
  --color-bg-muted: #e5ecfb;
  --color-bg-inset: #d7e1f4;
  --color-surface: #ffffff;
  --color-surface-raised: #fbfdff;
  --color-border: #d7e0f1;
  --color-border-subtle: #e7edf8;
  --color-text: #11172a;
  --color-text-secondary: #56617a;
  --color-text-muted: #7b849a;
  --color-primary: #4a5ee7;
  --color-primary-hover: #4052cf;
  --color-primary-subtle: rgba(74, 94, 231, 0.11);
  --color-primary-text: #ffffff;
  --color-secondary: #087ea4;
  --color-secondary-hover: #066985;
  --color-secondary-subtle: rgba(81, 190, 242, 0.16);
  --color-secondary-text: #ffffff;
  --color-accent: var(--color-primary);
  --color-accent-hover: var(--color-primary-hover);
  --color-accent-subtle: var(--color-primary-subtle);
  --color-accent-text: var(--color-primary-text);
  --color-success: #0f8a5f;
  --color-success-subtle: rgba(15, 138, 95, 0.11);
  --color-warning: #a86612;
  --color-warning-subtle: rgba(168, 102, 18, 0.12);
  --color-danger: #c73b45;
  --color-danger-subtle: rgba(199, 59, 69, 0.1);
  --color-message-sent-bg: color-mix(in srgb, var(--color-primary) 13%, var(--color-surface));
  --color-message-sent-border: color-mix(in srgb, var(--color-primary) 26%, var(--color-border));
  --color-message-sent-shadow: color-mix(in srgb, var(--color-primary) 12%, transparent);
  --color-message-received-bg: color-mix(in srgb, var(--color-bg-muted) 48%, var(--color-surface));
  --color-message-received-border: var(--color-border-subtle);
  --color-message-file-sent-bg: color-mix(in srgb, var(--color-surface-raised) 72%, var(--color-primary-subtle));
  --color-message-file-sent-border: color-mix(in srgb, var(--color-primary) 20%, var(--color-border-subtle));
  --color-message-file-received-bg: color-mix(in srgb, var(--color-surface) 74%, var(--color-bg-muted));
  --color-message-file-received-border: var(--color-border-subtle);
  --shadow-sm: 0 1px 2px rgba(15, 25, 54, 0.06);
  --shadow-md: 0 10px 28px rgba(15, 25, 54, 0.09);
  --shadow-lg: 0 18px 50px rgba(15, 25, 54, 0.18);
}

[data-theme='dark'] {
  color-scheme: dark;
  --color-bg: #090d18;
  --color-bg-subtle: #0f1424;
  --color-bg-muted: #171d31;
  --color-bg-inset: #242b42;
  --color-surface: #121829;
  --color-surface-raised: #202842;
  --color-border: #2c3654;
  --color-border-subtle: #222b45;
  --color-text: #f5f7ff;
  --color-text-secondary: #c6d0eb;
  --color-text-muted: #8995b5;
  --color-primary: #9cafee;
  --color-primary-hover: #bdc8ff;
  --color-primary-subtle: rgba(156, 175, 238, 0.16);
  --color-primary-text: #080d18;
  --color-secondary: #51d4c8;
  --color-secondary-hover: #8ce9df;
  --color-secondary-subtle: rgba(81, 212, 200, 0.14);
  --color-secondary-text: #06121f;
  --color-accent: var(--color-primary);
  --color-accent-hover: var(--color-primary-hover);
  --color-accent-subtle: var(--color-primary-subtle);
  --color-accent-text: var(--color-primary-text);
  --color-success: #5ee0a2;
  --color-success-subtle: rgba(94, 224, 162, 0.13);
  --color-warning: #f7bd53;
  --color-warning-subtle: rgba(247, 189, 83, 0.13);
  --color-danger: #ff7068;
  --color-danger-subtle: rgba(255, 112, 104, 0.13);
  --color-message-sent-bg: color-mix(in srgb, var(--color-primary) 18%, var(--color-surface));
  --color-message-sent-border: color-mix(in srgb, var(--color-primary) 30%, var(--color-border));
  --color-message-sent-shadow: color-mix(in srgb, var(--color-primary) 10%, transparent);
  --color-message-received-bg: color-mix(in srgb, var(--color-surface-raised) 48%, var(--color-surface));
  --color-message-received-border: var(--color-border-subtle);
  --color-message-file-sent-bg: color-mix(in srgb, var(--color-surface-raised) 76%, var(--color-primary-subtle));
  --color-message-file-sent-border: color-mix(in srgb, var(--color-primary) 24%, var(--color-border-subtle));
  --color-message-file-received-bg: color-mix(in srgb, var(--color-surface-raised) 64%, var(--color-surface));
  --color-message-file-received-border: var(--color-border-subtle);
  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.22);
  --shadow-md: 0 10px 30px rgba(0, 0, 0, 0.3);
  --shadow-lg: 0 22px 70px rgba(0, 0, 0, 0.5);
}

body {
  background:
    linear-gradient(180deg, color-mix(in srgb, var(--color-bg-subtle) 74%, transparent), transparent 360px),
    var(--color-bg);
}

#root { min-height: 100vh; min-height: 100dvh; height: 100dvh; overflow: hidden; display: flex; flex-direction: column; }
:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: var(--radius-sm); }
::-webkit-scrollbar { width: 8px; height: 8px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: var(--radius-full); }
::-webkit-scrollbar-thumb:hover { background: var(--color-text-muted); }
::selection { background: var(--color-accent-subtle); color: var(--color-text); }
code { font-family: var(--font-mono); font-size: 0.9em; }

@keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.45; } }
@keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }
@keyframes dialog-in { from { opacity: 0; transform: translateY(8px) scale(0.98); } to { opacity: 1; transform: translateY(0) scale(1); } }
@keyframes toast-enter { from { opacity: 0; transform: translateY(14px); } to { opacity: 1; transform: translateY(0); } }
@keyframes chat-bubble-enter { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: translateY(0); } }
@keyframes peer-banner-pulse { 0%, 100% { opacity: 0.85; transform: scale(1); } 50% { opacity: 1; transform: scale(1.08); } }
`,
    },
  ],
})
