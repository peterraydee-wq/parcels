package main

import (
	"os"
	"regexp"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
)

var ansiRe = regexp.MustCompile("\x1b\\[[0-9;:]*[A-Za-z]")

func plain(s string) string { return ansiRe.ReplaceAllString(s, "") }

func press(m *model, k string) {
	var msg tea.KeyPressMsg
	switch k {
	case "enter":
		msg = tea.KeyPressMsg{Code: tea.KeyEnter}
	case "esc":
		msg = tea.KeyPressMsg{Code: tea.KeyEscape}
	case "space":
		msg = tea.KeyPressMsg{Code: tea.KeySpace, Text: " "}
	case "down":
		msg = tea.KeyPressMsg{Code: tea.KeyDown}
	case "up":
		msg = tea.KeyPressMsg{Code: tea.KeyUp}
	case "tab":
		msg = tea.KeyPressMsg{Code: tea.KeyTab}
	default:
		r := []rune(k)[0]
		msg = tea.KeyPressMsg{Code: r, Text: k}
	}
	m.Update(msg)
}

func drain(m *model, j *Job) {
	for ev := range j.Ch {
		m.Update(evMsg{job: j.ID, ev: ev})
	}
	m.Update(evMsg{job: j.ID, closed: true})
}

func setup(t *testing.T) *model {
	core, err := findCore()
	if err != nil {
		t.Skip("no core: " + err.Error())
	}
	m := newModel(core)
	m.Update(tea.WindowSizeMsg{Width: 120, Height: 36})
	// providers
	for _, j := range m.jobs {
		drain(m, j)
	}
	m.splash = false
	return m
}

func TestFlow(t *testing.T) {
	os.Setenv("PARCELS_REPOS", t.TempDir())
	m := setup(t)
	m.Init() // starts providers again: harmless
	for _, j := range m.jobs {
		drain(m, j)
	}
	m.splash = false

	t.Log("\n" + plain(m.View().Content))

	// type a query and search
	for _, c := range "ripgrep" {
		press(m, string(c))
	}
	press(m, "enter")
	for _, j := range m.jobs {
		drain(m, j)
	}
	out := plain(m.View().Content)
	t.Log("\n" + out)
	if !strings.Contains(out, "ripgrep") {
		t.Fatal("no ripgrep in results")
	}
	// select first, ask to install -> confirm modal
	press(m, "space")
	press(m, "enter")
	if m.confirm == nil {
		t.Fatal("expected confirm")
	}
	t.Log("\n" + plain(m.View().Content))
	press(m, "n")
	if m.confirm != nil {
		t.Fatal("confirm should close")
	}
	// tabs
	for _, k := range []string{"tab", "tab", "tab"} {
		press(m, k)
		for _, j := range m.jobs {
			drain(m, j)
		}
		if len(m.View().Content) == 0 {
			t.Fatal("empty view")
		}
	}
	t.Log("\n" + plain(m.View().Content))
	// eggs
	press(m, "1")
	press(m, "/")
	m.input.SetValue("")
	for _, c := range "loomer" {
		press(m, string(c))
	}
	press(m, "enter")
	if m.overlay != "fox" {
		t.Fatalf("overlay=%q", m.overlay)
	}
	t.Log("\n" + plain(m.View().Content))
	press(m, "x")
	// konami
	for _, k := range []string{"up", "up", "down", "down", "left", "right", "left", "right", "b", "a"} {
		switch k {
		case "left":
			m.Update(tea.KeyPressMsg{Code: tea.KeyLeft})
		case "right":
			m.Update(tea.KeyPressMsg{Code: tea.KeyRight})
		default:
			press(m, k)
		}
	}
	if m.overlay != "konami" {
		t.Fatalf("konami overlay=%q keys=%v", m.overlay, m.keys)
	}
	_ = m.View().Content
}

func TestTaskLifecycle(t *testing.T) {
	dir := t.TempDir()
	os.Setenv("PARCELS_REPOS", dir)
	m := setup(t)
	it := Item{Kind: "result", Provider: "git", ID: "octocat/Hello-World", Cmd: "git clone"}
	cmd := m.enqueue("install", []Item{it})
	if cmd == nil || m.running == nil {
		t.Fatal("task should be running")
	}
	for _, j := range m.jobs {
		drain(m, j)
	}
	if m.tasks[0].State != "ok" {
		t.Fatalf("state=%s lines=%v", m.tasks[0].State, m.tasks[0].Lines)
	}
	t.Log("\n" + plain(m.View().Content))
}
