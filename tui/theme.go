package main

// everforest, with forest green doing the heavy lifting and loomer's orange as the accent.

import (
	"image/color"
	"math"
	"os"
	"strings"
	"unicode"

	"charm.land/lipgloss/v2"
	"github.com/charmbracelet/x/ansi"
)

var (
	cBgDim   = lipgloss.Color("#232a2e")
	cBg      = lipgloss.Color("#2d353b")
	cBg1     = lipgloss.Color("#343f44")
	cBg2     = lipgloss.Color("#3d484d")
	cBgGreen = lipgloss.Color("#3c4841") // selection
	cFg      = lipgloss.Color("#d3c6aa")
	cGrey0   = lipgloss.Color("#7a8478")
	cGrey1   = lipgloss.Color("#859289")
	cGrey2   = lipgloss.Color("#9da9a0")
	cForest  = lipgloss.Color("#3f7d4e") // the deep forest green: borders, pills
	cGreen   = lipgloss.Color("#a7c080")
	cAqua    = lipgloss.Color("#83c092")
	cBlue    = lipgloss.Color("#7fbbb3")
	cYellow  = lipgloss.Color("#dbbc7f")
	cOrange  = lipgloss.Color("#e69875") // loomer
	cRed     = lipgloss.Color("#e67e80")
	cPurple  = lipgloss.Color("#d699b6")
)

var provColor = map[string]color.Color{
	"winget": cBlue, "scoop": cGreen, "choco": cOrange, "npm": cRed, "pnpm": cYellow,
	"bun": cFg, "pip": cYellow, "uv": cPurple, "pipx": cAqua, "cargo": cOrange,
	"go": cBlue, "vcpkg": cPurple, "dotnet": cPurple, "git": cRed,
}

// canonical display order
var provOrder = []string{"winget", "scoop", "choco", "npm", "pnpm", "bun", "pip", "uv", "pipx", "cargo", "go", "vcpkg", "dotnet", "git"}

func provRank(id string) int {
	for i, p := range provOrder {
		if p == id {
			return i
		}
	}
	return len(provOrder)
}

func colorFor(id string) color.Color {
	if c, ok := provColor[id]; ok {
		return c
	}
	return cGrey2
}

// paint renders text with optional fg/bg/bold. nil colors are skipped.
func paint(s string, fg, bg color.Color, bold bool) string {
	if s == "" {
		return ""
	}
	st := lipgloss.NewStyle()
	if fg != nil {
		st = st.Foreground(fg)
	}
	if bg != nil {
		st = st.Background(bg)
	}
	if bold {
		st = st.Bold(true)
	}
	return st.Render(s)
}

func fg(c color.Color, s string) string { return paint(s, c, nil, false) }

// fit truncates (with an ellipsis) or right-pads to exactly w cells.
func fit(s string, w int) string {
	if w <= 0 {
		return ""
	}
	t := ansi.Truncate(s, w, "…")
	if d := w - ansi.StringWidth(t); d > 0 {
		t += strings.Repeat(" ", d)
	}
	return t
}

func padTo(s string, w int) string {
	if d := w - ansi.StringWidth(s); d > 0 {
		return s + strings.Repeat(" ", d)
	}
	return s
}

// sanitize strips control characters/escapes from anything that came from outside
// (package descriptions are untrusted text; they must never reach the terminal as codes).
func sanitize(s string) string {
	return strings.Map(func(r rune) rune {
		switch {
		case r == '\t':
			return ' '
		case unicode.IsControl(r):
			return -1
		case r >= 0x202a && r <= 0x202e, r >= 0x2066 && r <= 0x2069: // bidi overrides
			return -1
		}
		return r
	}, s)
}

func wrap(s string, w int) []string {
	if w < 4 {
		return []string{s}
	}
	var out []string
	line := ""
	for _, word := range strings.Fields(s) {
		for ansi.StringWidth(word) > w {
			cut := ansi.Truncate(word, w, "")
			if line != "" {
				out = append(out, line)
				line = ""
			}
			out = append(out, cut)
			word = strings.TrimPrefix(word, cut)
		}
		switch {
		case line == "":
			line = word
		case ansi.StringWidth(line)+1+ansi.StringWidth(word) <= w:
			line += " " + word
		default:
			out = append(out, line)
			line = word
		}
	}
	if line != "" {
		out = append(out, line)
	}
	return out
}

var spinFrames = []string{"⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"}

func spin(frame int) string { return spinFrames[frame%len(spinFrames)] }

// waveform: a little ascii vu meter, because sound deserves a picture.
func waveform(frame, n int) string {
	bars := []rune("▁▂▃▄▅▆▇█")
	var b strings.Builder
	for i := 0; i < n; i++ {
		v := (math.Sin(float64(frame)*0.35+float64(i)*0.9)+math.Sin(float64(frame)*0.17+float64(i)*1.7))/4 + 0.5
		idx := int(v * float64(len(bars)))
		if idx < 0 {
			idx = 0
		}
		if idx >= len(bars) {
			idx = len(bars) - 1
		}
		b.WriteRune(bars[idx])
	}
	return b.String()
}

// the parcels glyph: nf-md-package (U+F03D6). needs a Nerd Font; set PARCELS_ASCII=1 for a plain fallback.
const pkgGlyph = "\U000F03D6"

var useNerd = os.Getenv("PARCELS_ASCII") == ""

func icon() string {
	if useNerd {
		return pkgGlyph
	}
	return "▣"
}
