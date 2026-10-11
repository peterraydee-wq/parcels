package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"sort"
	"strings"
	"time"

	"charm.land/bubbles/v2/textinput"
	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
)

type tabID int

const (
	tabSearch tabID = iota
	tabInstalled
	tabUpdates
	tabProviders
)

var tabNames = []string{"Search", "Installed", "Updates", "Providers"}

type Item struct {
	Kind, Provider, ID, Name, Version, Latest, Desc, Cmd, Cmd2 string
	Tier, Score                                                int // search ranking, see fuzzy.go
}

func itemFrom(e Event) Item {
	return Item{Kind: e.Type, Provider: e.Provider, ID: e.ID, Name: e.Name, Version: e.Version,
		Latest: e.Latest, Desc: e.Desc, Cmd: e.Cmd, Cmd2: e.Cmd2}
}

func (i Item) key() string { return i.Provider + "\x00" + i.ID }
func (i Item) title() string {
	if i.Name != "" {
		return i.Name
	}
	return i.ID
}

// the command preview for an action on this item (core computed them already)
func (i Item) cmdFor(action string) string {
	switch i.Kind {
	case "result":
		if action == "install" {
			return i.Cmd
		}
	case "installed":
		if action == "uninstall" {
			return i.Cmd
		}
		if action == "upgrade" {
			return i.Cmd2
		}
	case "outdated":
		if action == "upgrade" {
			return i.Cmd
		}
		if action == "uninstall" {
			return i.Cmd2
		}
	}
	return ""
}

func allowed(kind, action string) bool {
	switch kind {
	case "result":
		return action == "install"
	case "installed", "outdated":
		return action == "uninstall" || action == "upgrade"
	}
	return false
}

type pstat struct {
	state string // running, done, error, skipped
	count int
	ms    int
	err   string
}

type listState struct {
	items   []Item
	sel     map[string]bool
	cur     int
	top     int
	stat    map[string]*pstat
	job     *Job
	loaded  bool
	loading bool
	query   string // search term (search tab) or local filter (others)
	ran     bool
	errmsg  string
}

func newList() *listState {
	return &listState{sel: map[string]bool{}, stat: map[string]*pstat{}}
}

type logLine struct {
	text     string
	err      bool
	progress bool
}

type Task struct {
	N      int
	Action string
	Item   Item
	Cmd    string
	Lines  []logLine
	State  string // queued, running, ok, fail
	Code   int
}

func (t *Task) label() string {
	verb := map[string]string{"install": "install", "uninstall": "remove", "upgrade": "upgrade"}[t.Action]
	return verb + " " + t.Item.ID + " (" + t.Item.Provider + ")"
}

type confirmState struct {
	action string
	items  []Item
}

type tickMsg time.Time
type exportMsg struct {
	path string
	n    int
	err  error
}

type model struct {
	core *Core
	cfg  Config
	w, h int
	tab  tabID

	input     textinput.Model
	filter    textinput.Model
	focusIn   bool // search box focused
	filtering bool // local filter focused

	lists       [3]*listState
	provs       []Event
	provsLoaded bool
	provCur     int

	jobs    map[int]*Job
	nextJob int

	tasks   []*Task
	nextN   int
	dock    bool
	running *Task

	confirm *confirmState
	overlay string
	frame   int
	splash  bool

	notice   string
	noticeAt int

	keys     []string
	quitting bool
}

func newInput(ph string) textinput.Model {
	ti := textinput.New()
	ti.Prompt = ""
	ti.Placeholder = ph
	ti.CharLimit = 80
	st := textinput.DefaultDarkStyles()
	st.Focused.Text = lipgloss.NewStyle().Foreground(cFg)
	st.Focused.Placeholder = lipgloss.NewStyle().Foreground(cGrey0)
	st.Blurred.Text = lipgloss.NewStyle().Foreground(cGrey2)
	st.Blurred.Placeholder = lipgloss.NewStyle().Foreground(cGrey0)
	st.Cursor.Color = cGreen
	ti.SetStyles(st)
	return ti
}

func newModel(core *Core) *model {
	m := &model{
		core:    core,
		cfg:     loadConfig(),
		input:   newInput("search every package manager at once…"),
		filter:  newInput("filter…"),
		jobs:    map[int]*Job{},
		dock:    true,
		splash:  true,
		focusIn: true,
	}
	for i := range m.lists {
		m.lists[i] = newList()
	}
	return m
}

func tickCmd() tea.Cmd {
	return tea.Tick(110*time.Millisecond, func(t time.Time) tea.Msg { return tickMsg(t) })
}

func (m *model) Init() tea.Cmd {
	blink := m.input.Focus()
	cmds := []tea.Cmd{tickCmd(), blink}
	if c := m.startProviders(); c != nil {
		cmds = append(cmds, c)
	}
	if !m.cfg.Mute {
		playTones(toneBoot)
	}
	return tea.Batch(cmds...)
}

func (m *model) notify(s string) {
	m.notice = s
	m.noticeAt = m.frame
}

// ---------------------------------------------------------------- jobs

func (m *model) register(kind string, ch chan Event, cancel func()) *Job {
	m.nextJob++
	j := &Job{ID: m.nextJob, Kind: kind, Ch: ch, cancel: cancel}
	m.jobs[j.ID] = j
	return j
}

func (m *model) startProviders() tea.Cmd {
	ch, cancel, err := m.core.Start("providers")
	if err != nil {
		m.notify("can't start parcels-core: " + err.Error())
		m.provsLoaded = true
		return nil
	}
	m.provs = nil
	j := m.register("providers", ch, cancel)
	return waitEv(j)
}

func (m *model) enabledIDs() []string {
	var ids []string
	for _, p := range m.provs {
		if p.Available && !m.cfg.isDisabled(p.ID) {
			ids = append(ids, p.ID)
		}
	}
	return ids
}

func (m *model) startList(ls *listState, kind, q string) tea.Cmd {
	if ls.job != nil {
		ls.job.cancel()
		delete(m.jobs, ls.job.ID)
		ls.job = nil
	}
	ids := m.enabledIDs()
	if len(ids) == 0 {
		m.notify("no package managers enabled — see the Providers tab")
		return nil
	}
	args := []string{kind}
	if kind == "search" {
		args = append(args, strings.TrimLeft(q, "- "))
	}
	args = append(args, "--only", strings.Join(ids, ","))
	ch, cancel, err := m.core.Start(args...)
	if err != nil {
		ls.errmsg = err.Error()
		return nil
	}
	ls.items, ls.cur, ls.top, ls.errmsg = nil, 0, 0, ""
	ls.sel = map[string]bool{}
	ls.stat = map[string]*pstat{}
	for _, id := range ids {
		ls.stat[id] = &pstat{state: "running"}
	}
	ls.loading, ls.ran = true, true
	if kind == "search" {
		ls.query = q
	}
	j := m.register(kind, ch, cancel)
	j.ls = ls
	ls.job = j
	return waitEv(j)
}

func (m *model) cancelAll() {
	for _, j := range m.jobs {
		j.cancel()
	}
}

// ---------------------------------------------------------------- lists

func (ls *listState) resortSearch() {
	cur := ""
	if ls.cur < len(ls.items) {
		cur = ls.items[ls.cur].key()
	}
	sort.SliceStable(ls.items, func(a, b int) bool {
		x, y := ls.items[a], ls.items[b]
		if x.Tier != y.Tier {
			return x.Tier < y.Tier
		}
		if x.Score != y.Score {
			return x.Score > y.Score
		}
		if ra, rb := provRank(x.Provider), provRank(y.Provider); ra != rb {
			return ra < rb
		}
		return len(x.title()) < len(y.title())
	})
	for i, it := range ls.items {
		if it.key() == cur {
			ls.cur = i
			break
		}
	}
}

func (ls *listState) sortByProvider() {
	sort.SliceStable(ls.items, func(a, b int) bool {
		ra, rb := provRank(ls.items[a].Provider), provRank(ls.items[b].Provider)
		if ra != rb {
			return ra < rb
		}
		return strings.ToLower(ls.items[a].title()) < strings.ToLower(ls.items[b].title())
	})
}

func (m *model) list() *listState {
	if m.tab > tabUpdates {
		return nil
	}
	return m.lists[m.tab]
}

func (m *model) visible() []Item {
	ls := m.list()
	if ls == nil {
		return nil
	}
	if m.tab == tabSearch || strings.TrimSpace(ls.query) == "" {
		return ls.items
	}
	q := strings.ToLower(strings.TrimSpace(ls.query))
	var out []Item
	for _, it := range ls.items {
		if strings.Contains(strings.ToLower(it.ID+" "+it.Name+" "+it.Provider+" "+it.Desc), q) {
			out = append(out, it)
		}
	}
	return out
}

func (m *model) clamp() {
	ls := m.list()
	if ls == nil {
		return
	}
	n := len(m.visible())
	if ls.cur >= n {
		ls.cur = n - 1
	}
	if ls.cur < 0 {
		ls.cur = 0
	}
	rows := m.layout().bodyH - 2
	if rows < 1 {
		rows = 1
	}
	if ls.cur < ls.top {
		ls.top = ls.cur
	}
	if ls.cur >= ls.top+rows {
		ls.top = ls.cur - rows + 1
	}
	if ls.top < 0 {
		ls.top = 0
	}
}

func (m *model) current() (Item, bool) {
	ls := m.list()
	vis := m.visible()
	if ls == nil || len(vis) == 0 || ls.cur >= len(vis) {
		return Item{}, false
	}
	return vis[ls.cur], true
}

func (m *model) targets() []Item {
	ls := m.list()
	if ls == nil {
		return nil
	}
	var out []Item
	for _, it := range m.visible() {
		if ls.sel[it.key()] {
			out = append(out, it)
		}
	}
	if len(out) == 0 {
		if it, ok := m.current(); ok {
			out = append(out, it)
		}
	}
	return out
}

// ---------------------------------------------------------------- layout

type layout struct {
	inputY, chipsY, bodyY, bodyH, dockH, listW, detailW int
}

func (m *model) layout() layout {
	l := layout{inputY: 3, chipsY: 4, bodyY: 5}
	if len(m.tasks) > 0 && m.dock {
		l.dockH = 8
	}
	l.bodyH = m.h - l.bodyY - l.dockH - 1
	if l.bodyH < 5 {
		l.bodyH = 5
	}
	if m.w >= 100 && m.tab != tabProviders {
		l.detailW = m.w * 2 / 5
		l.listW = m.w - l.detailW
	} else {
		l.listW = m.w
	}
	return l
}

// ---------------------------------------------------------------- tasks

func (m *model) enqueue(action string, items []Item) tea.Cmd {
	for _, it := range items {
		m.nextN++
		m.tasks = append(m.tasks, &Task{N: m.nextN, Action: action, Item: it, State: "queued", Cmd: it.cmdFor(action)})
	}
	m.dock = true
	return m.pump()
}

func (m *model) pump() tea.Cmd {
	if m.running != nil {
		return nil
	}
	for _, t := range m.tasks {
		if t.State != "queued" {
			continue
		}
		ch, cancel, err := m.core.Start("run", t.Action, t.Item.Provider, t.Item.ID)
		if err != nil {
			t.State = "fail"
			t.Lines = append(t.Lines, logLine{text: err.Error(), err: true})
			continue
		}
		t.State = "running"
		m.running = t
		j := m.register("task", ch, cancel)
		j.task = t
		return waitEv(j)
	}
	return nil
}

func (m *model) queueDone() {
	ok, bad := 0, 0
	for _, t := range m.tasks {
		switch t.State {
		case "ok":
			ok++
		case "fail":
			bad++
		}
	}
	if bad == 0 {
		m.notify("all done — " + plural(ok, "task") + " finished")
		if !m.cfg.Mute {
			playTones(toneOK)
		}
	} else {
		m.notify(plural(bad, "task") + " failed — see the log")
		if !m.cfg.Mute {
			playTones(toneFail)
		}
	}
	// installed/updates are stale now
	for _, i := range []int{int(tabInstalled), int(tabUpdates)} {
		m.lists[i].loaded = false
	}
}

func plural(n int, w string) string {
	if n == 1 {
		return "1 " + w
	}
	return itoa(n) + " " + w + "s"
}

func itoa(n int) string {
	b, _ := json.Marshal(n)
	return string(b)
}

func (m *model) anyBusy() bool {
	for _, t := range m.tasks {
		if t.State == "queued" || t.State == "running" {
			return true
		}
	}
	return false
}

func (m *model) logTail(t *Task, n int) []logLine {
	if len(t.Lines) <= n {
		return t.Lines
	}
	return t.Lines[len(t.Lines)-n:]
}

// ---------------------------------------------------------------- export

func exportCmd(c *Core, only []string) tea.Cmd {
	return func() tea.Msg {
		args := []string{"export"}
		if len(only) > 0 {
			args = append(args, "--only", strings.Join(only, ","))
		}
		cmd := exec.Command(c.path, args...)
		hideWindow(cmd)
		out, err := cmd.Output()
		if err != nil {
			return exportMsg{err: err}
		}
		var doc struct {
			Packages []json.RawMessage `json:"packages"`
		}
		_ = json.Unmarshal(bytes.TrimSpace(out), &doc)
		name := "parcels-manifest.json"
		if err := os.WriteFile(name, out, 0o644); err != nil {
			return exportMsg{err: err}
		}
		return exportMsg{path: name, n: len(doc.Packages)}
	}
}
