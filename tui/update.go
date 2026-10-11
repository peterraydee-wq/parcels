package main

import (
	"strings"

	tea "charm.land/bubbletea/v2"
)

func (m *model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.w, m.h = msg.Width, msg.Height
		m.input.SetWidth(max(10, min(60, m.w-24)))
		m.filter.SetWidth(max(10, min(60, m.w-24)))
		m.clamp()
		return m, nil

	case tickMsg:
		m.frame++
		if m.splash && m.provsLoaded && m.frame >= 9 {
			m.splash = false
		}
		if m.notice != "" && m.frame-m.noticeAt > 45 {
			m.notice = ""
		}
		return m, tickCmd()

	case evMsg:
		return m.onEvent(msg)

	case exportMsg:
		if msg.err != nil {
			m.notify("export failed: " + msg.err.Error())
		} else {
			m.notify("wrote " + msg.path + " (" + plural(msg.n, "package") + ") — restore with: parcels import " + msg.path)
		}
		return m, nil

	case tea.MouseWheelMsg:
		return m.onWheel(msg)

	case tea.MouseClickMsg:
		return m.onClick(msg)

	case tea.KeyPressMsg:
		return m.onKey(msg)
	}

	// everything else (cursor blink etc.) goes to whichever input is live
	var cmd tea.Cmd
	if m.filtering {
		m.filter, cmd = m.filter.Update(msg)
	} else if m.focusIn && m.tab == tabSearch {
		m.input, cmd = m.input.Update(msg)
	}
	return m, cmd
}

// ---------------------------------------------------------------- core events

func (m *model) onEvent(msg evMsg) (tea.Model, tea.Cmd) {
	j := m.jobs[msg.job]
	if j == nil {
		return m, nil
	}
	if msg.closed {
		delete(m.jobs, j.ID)
		return m, m.jobClosed(j)
	}
	ev := msg.ev
	switch j.Kind {
	case "providers":
		switch ev.Type {
		case "provider":
			m.provs = append(m.provs, ev)
		case "done":
			sortProvs(m.provs)
			m.provsLoaded = true
		}
	case "search", "list", "outdated":
		m.onListEvent(j, ev)
	case "task":
		m.onTaskEvent(j.task, ev)
	}
	return m, waitEv(j)
}

func sortProvs(ps []Event) {
	for i := 1; i < len(ps); i++ {
		for k := i; k > 0 && provRank(ps[k].ID) < provRank(ps[k-1].ID); k-- {
			ps[k], ps[k-1] = ps[k-1], ps[k]
		}
	}
}

func (m *model) onListEvent(j *Job, ev Event) {
	ls := j.ls
	switch ev.Type {
	case "start":
		if st := ls.stat[ev.Provider]; st != nil {
			st.state = "running"
		}
	case "result", "installed", "outdated":
		it := itemFrom(ev)
		if j.Kind == "search" {
			rankItem(&it, ls.query)
		}
		ls.items = append(ls.items, it)
		if j.Kind == "search" {
			ls.resortSearch()
		}
	case "end":
		st := ls.stat[ev.Provider]
		if st == nil {
			st = &pstat{}
			ls.stat[ev.Provider] = st
		}
		st.count, st.ms, st.err = ev.Count, ev.Ms, ev.Error
		switch {
		case ev.Skipped:
			st.state = "skipped"
		case ev.Error != "":
			st.state = "error"
		default:
			st.state = "done"
		}
	case "done":
		ls.loading = false
		ls.loaded = true
		ls.job = nil
		if ev.Error != "" {
			ls.errmsg = ev.Error
		}
		if j.Kind != "search" {
			ls.sortByProvider()
		}
	}
	m.clamp()
}

func (m *model) onTaskEvent(t *Task, ev Event) {
	switch ev.Type {
	case "plan":
		t.Cmd = ev.Cmd
	case "line":
		ll := logLine{text: ev.Text, err: ev.Stream == "err", progress: ev.Progress}
		if n := len(t.Lines); n > 0 && t.Lines[n-1].progress && ll.progress {
			t.Lines[n-1] = ll
		} else {
			t.Lines = append(t.Lines, ll)
		}
		if len(t.Lines) > 400 {
			t.Lines = t.Lines[len(t.Lines)-300:]
		}
	case "exit":
		t.Code = ev.Code
		if ev.OK {
			t.State = "ok"
		} else {
			t.State = "fail"
		}
	}
}

func (m *model) jobClosed(j *Job) tea.Cmd {
	switch j.Kind {
	case "task":
		t := j.task
		if t.State == "running" { // core died without an exit event
			t.State = "fail"
			t.Lines = append(t.Lines, logLine{text: "parcels-core exited unexpectedly", err: true})
		}
		m.running = nil
		next := m.pump()
		if next == nil && !m.anyBusy() {
			m.queueDone()
			if r := m.reloadStale(); r != nil {
				return r
			}
		}
		return next
	case "search", "list", "outdated":
		if j.ls != nil && j.ls.job == j {
			j.ls.loading = false
			j.ls.job = nil
		}
	}
	return nil
}

func (m *model) reloadStale() tea.Cmd {
	switch m.tab {
	case tabInstalled:
		return m.startList(m.lists[tabInstalled], "list", "")
	case tabUpdates:
		return m.startList(m.lists[tabUpdates], "outdated", "")
	}
	return nil
}

// ---------------------------------------------------------------- mouse

func (m *model) onWheel(msg tea.MouseWheelMsg) (tea.Model, tea.Cmd) {
	if m.overlay != "" || m.confirm != nil || m.splash {
		return m, nil
	}
	d := 0
	switch msg.Mouse().Button {
	case tea.MouseWheelUp:
		d = -3
	case tea.MouseWheelDown:
		d = 3
	}
	m.move(d)
	return m, nil
}

func (m *model) onClick(msg tea.MouseClickMsg) (tea.Model, tea.Cmd) {
	if m.overlay != "" || m.confirm != nil || m.splash {
		return m, nil
	}
	mo := msg.Mouse()
	if mo.Button != tea.MouseLeft {
		return m, nil
	}
	if mo.Y == 1 { // tab bar
		for i, r := range m.tabRanges() {
			if mo.X >= r[0] && mo.X < r[1] {
				return m, m.switchTab(tabID(i))
			}
		}
		return m, nil
	}
	l := m.layout()
	if mo.Y > l.bodyY && mo.Y < l.bodyY+l.bodyH-1 && mo.X < l.listW {
		row := mo.Y - l.bodyY - 1
		if ls := m.list(); ls != nil {
			ls.cur = ls.top + row
			m.clamp()
			m.focusIn, m.filtering = false, false
		} else if m.tab == tabProviders {
			if idx := row; idx >= 0 && idx < len(m.provs) {
				m.provCur = idx
			}
		}
	}
	return m, nil
}

// ---------------------------------------------------------------- keys

func (m *model) switchTab(t tabID) tea.Cmd {
	m.tab = t
	m.filtering = false
	m.focusIn = false
	var cmds []tea.Cmd
	switch t {
	case tabSearch:
		m.focusIn = len(m.lists[tabSearch].items) == 0
		if m.focusIn {
			cmds = append(cmds, m.input.Focus())
		}
	case tabInstalled:
		if ls := m.lists[t]; !ls.loaded && !ls.loading {
			cmds = append(cmds, m.startList(ls, "list", ""))
		}
	case tabUpdates:
		if ls := m.lists[t]; !ls.loaded && !ls.loading {
			cmds = append(cmds, m.startList(ls, "outdated", ""))
		}
	}
	m.clamp()
	return tea.Batch(cmds...)
}

func (m *model) move(d int) {
	switch m.tab {
	case tabProviders:
		m.provCur = max(0, min(len(m.provs)-1, m.provCur+d))
	default:
		if ls := m.list(); ls != nil {
			ls.cur += d
			m.clamp()
		}
	}
}

func (m *model) onKey(msg tea.KeyPressMsg) (tea.Model, tea.Cmd) {
	k := msg.String()

	if m.splash {
		m.splash = false
		if k == "ctrl+c" {
			return m, m.quit()
		}
		return m, nil
	}
	if k == "ctrl+c" {
		return m, m.quit()
	}

	// overlays (help, eggs): any key closes
	if m.overlay != "" {
		m.overlay = ""
		return m, nil
	}

	// konami: tracked before anything can swallow the keys
	m.keys = append(m.keys, k)
	if len(m.keys) > 10 {
		m.keys = m.keys[len(m.keys)-10:]
	}
	if strings.Join(m.keys, ",") == konami {
		m.keys = nil
		if m.focusIn { // the b and a landed in the box
			v := m.input.Value()
			m.input.SetValue(strings.TrimSuffix(v, "ba"))
		}
		m.overlay = "konami"
		if !m.cfg.Mute {
			playTones(toneOK)
		}
		return m, nil
	}

	if m.confirm != nil {
		switch k {
		case "y", "Y", "enter":
			c := m.confirm
			m.confirm = nil
			return m, m.enqueue(c.action, c.items)
		case "n", "N", "esc", "q":
			m.confirm = nil
		}
		return m, nil
	}

	// text entry
	if m.filtering {
		switch k {
		case "enter":
			m.filtering = false
			m.filter.Blur()
			return m, nil
		case "esc":
			m.filtering = false
			m.filter.Blur()
			m.filter.SetValue("")
			m.list().query = ""
			m.clamp()
			return m, nil
		}
		var cmd tea.Cmd
		m.filter, cmd = m.filter.Update(msg)
		m.list().query = m.filter.Value()
		m.clamp()
		return m, cmd
	}
	if m.focusIn && m.tab == tabSearch {
		switch k {
		case "enter":
			return m, m.submitSearch()
		case "esc", "down":
			if len(m.lists[tabSearch].items) > 0 || k == "esc" {
				m.focusIn = false
				m.input.Blur()
			}
			return m, nil
		case "tab", "shift+tab":
			// fall through to the global handler
		default:
			var cmd tea.Cmd
			m.input, cmd = m.input.Update(msg)
			return m, cmd
		}
	}

	switch k {
	case "q":
		if m.anyBusy() {
			m.notify("a task is running — ctrl+c to force quit")
			return m, nil
		}
		return m, m.quit()
	case "tab", "right", "l":
		return m, m.switchTab((m.tab + 1) % 4)
	case "shift+tab", "left", "h":
		return m, m.switchTab((m.tab + 3) % 4)
	case "1", "2", "3", "4":
		return m, m.switchTab(tabID(k[0] - '1'))
	case "?":
		m.overlay = "help"
	case "t":
		m.dock = !m.dock
		m.clamp()
	case "m":
		m.cfg.Mute = !m.cfg.Mute
		m.cfg.save()
		if m.cfg.Mute {
			m.notify("sound off")
		} else {
			m.notify("sound on")
			playTones(toneBoot)
		}
	case "e":
		m.notify("exporting manifest…")
		return m, exportCmd(m.core, m.enabledIDs())
	case "up", "k":
		m.move(-1)
	case "down", "j":
		m.move(1)
	case "pgup":
		m.move(-10)
	case "pgdown":
		m.move(10)
	case "home", "g":
		m.move(-1 << 20)
	case "end", "G":
		m.move(1 << 20)
	}

	if m.tab == tabProviders {
		return m, m.providerKey(k)
	}
	return m, m.listKey(k)
}

func (m *model) quit() tea.Cmd {
	m.quitting = true
	m.cancelAll()
	return tea.Quit
}

func (m *model) providerKey(k string) tea.Cmd {
	switch k {
	case "space", "enter":
		if m.provCur < len(m.provs) {
			p := m.provs[m.provCur]
			if !p.Available {
				h := p.Hint
				if h == "" {
					h = "not found on PATH or in common install dirs"
				}
				m.notify(p.Name + " not found — " + h)
				return nil
			}
			m.cfg.toggle(p.ID)
			m.cfg.save()
			state := "on"
			if m.cfg.isDisabled(p.ID) {
				state = "off"
			}
			m.notify(p.Name + " " + state)
			m.lists[tabInstalled].loaded = false
			m.lists[tabUpdates].loaded = false
		}
	case "r", "ctrl+r":
		m.notify("re-checking package managers…")
		return m.startProviders()
	}
	return nil
}

func (m *model) listKey(k string) tea.Cmd {
	ls := m.list()
	if ls == nil {
		return nil
	}
	switch k {
	case "/":
		if m.tab == tabSearch {
			m.focusIn = true
			return m.input.Focus()
		}
		m.filtering = true
		m.filter.SetValue(ls.query)
		return m.filter.Focus()
	case "space":
		if it, ok := m.current(); ok {
			if ls.sel[it.key()] {
				delete(ls.sel, it.key())
			} else {
				ls.sel[it.key()] = true
			}
			m.move(1)
		}
	case "a":
		for _, it := range m.visible() {
			ls.sel[it.key()] = true
		}
	case "A":
		ls.sel = map[string]bool{}
	case "r", "ctrl+r":
		switch m.tab {
		case tabSearch:
			if strings.TrimSpace(ls.query) != "" {
				return m.startList(ls, "search", ls.query)
			}
		case tabInstalled:
			return m.startList(ls, "list", "")
		case tabUpdates:
			return m.startList(ls, "outdated", "")
		}
	case "enter":
		switch m.tab {
		case tabSearch:
			return m.ask("install")
		case tabUpdates:
			return m.ask("upgrade")
		case tabInstalled:
			return m.ask("upgrade")
		}
	case "i":
		return m.ask("install")
	case "u":
		return m.ask("upgrade")
	case "x", "delete":
		return m.ask("uninstall")
	case "U":
		if m.tab == tabUpdates {
			ls.sel = map[string]bool{}
			for _, it := range m.visible() {
				ls.sel[it.key()] = true
			}
			return m.ask("upgrade")
		}
	}
	return nil
}

func (m *model) ask(action string) tea.Cmd {
	var ok []Item
	skipped := 0
	for _, it := range m.targets() {
		if allowed(it.Kind, action) {
			ok = append(ok, it)
		} else {
			skipped++
		}
	}
	if len(ok) == 0 {
		switch action {
		case "install":
			m.notify("that's already installed — try u to upgrade or x to remove")
		default:
			m.notify("nothing here to " + action + " — look on the Installed tab")
		}
		return nil
	}
	if skipped > 0 {
		m.notify(plural(skipped, "item") + " skipped (can't " + action + ")")
	}
	m.confirm = &confirmState{action: action, items: ok}
	return nil
}

func (m *model) submitSearch() tea.Cmd {
	q := strings.TrimSpace(m.input.Value())
	if q == "" {
		return nil
	}
	if egg, ok := eggs[strings.ToLower(q)]; ok {
		m.overlay = egg
		m.input.SetValue("")
		return nil
	}
	m.focusIn = false
	m.input.Blur()
	return m.startList(m.lists[tabSearch], "search", q)
}

func (m *model) tabRanges() [][2]int {
	var out [][2]int
	x := 1
	for i := range tabNames {
		w := len([]rune(m.tabLabel(tabID(i)))) + 2
		out = append(out, [2]int{x, x + w})
		x += w + 1
	}
	return out
}
