package main

import (
	"strings"
	"testing"
)

func TestGlyphAndHints(t *testing.T) {
	m := setup(t)
	m.Init()
	for _, j := range m.jobs {
		drain(m, j)
	}
	m.splash = false
	out := plain(m.View().Content)
	if !strings.Contains(out, "\U000F03D6 parcels") {
		t.Fatal("header should carry the 󰏖 glyph")
	}
	press(m, "esc") // search box has focus on launch
	press(m, "4")
	out = plain(m.View().Content)
	if !strings.Contains(out, "not found") || !strings.Contains(out, "→ ") {
		t.Fatalf("providers tab should show hints for missing managers:\n%s", out)
	}
	useNerd = false
	if icon() != "▣" {
		t.Fatal("ascii fallback")
	}
	useNerd = true
}
