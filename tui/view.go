package main

import (
	"image/color"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
	"github.com/charmbracelet/x/ansi"
)

func (m *model) View() tea.View {
	var s string
	switch {
	case m.w < 64 || m.h < 18:
		s = "parcels needs a window of at least 64×18.\nresize and it will wake up."
	case m.splash:
		s = m.viewSplash()
	case m.overlay != "":
		s = m.viewOverlay()
	case m.confirm != nil:
		s = m.viewConfirm()
	default:
		s = m.viewMain()
	}
	v := tea.NewView(s)
	v.AltScreen = true
	v.MouseMode = tea.MouseModeCellMotion
	v.WindowTitle = "parcels"
	v.BackgroundColor = cBg
	v.ForegroundColor = cFg
	return v
}

// ---------------------------------------------------------------- box

func box(title, right string, lines []string, w, h int, bc color.Color) string {
	if w < 6 || h < 3 {
		return ""
	}
	inner := w - 4
	top := "╭─"
	topTitle := ""
	if title != "" {
		topTitle = " " + title + " "
	}
	tw := ansi.StringWidth(topTitle)
	rw := 0
	rt := ""
	if right != "" {
		rt = " " + right + " "
		rw = ansi.StringWidth(rt)
	}
	fill := w - 2 - 1 - tw - rw - 1
	if fill < 0 {
		fill = 0
		topTitle = ansi.Truncate(topTitle, max(0, w-6), "…")
		tw = ansi.StringWidth(topTitle)
		rt, rw = "", 0
		fill = max(0, w-2-1-tw-1)
	}
	var b strings.Builder
	b.WriteString(fg(bc, top))
	b.WriteString(paint(topTitle, cGreen, nil, true))
	b.WriteString(fg(bc, strings.Repeat("─", fill)))
	b.WriteString(fg(cGrey1, rt))
	b.WriteString(fg(bc, "─╮"))
	b.WriteString("\n")
	for i := 0; i < h-2; i++ {
		line := ""
		if i < len(lines) {
			line = lines[i]
		}
		b.WriteString(fg(bc, "│ "))
		b.WriteString(fit(line, inner))
		b.WriteString(fg(bc, " │"))
		b.WriteString("\n")
	}
	b.WriteString(fg(bc, "╰"+strings.Repeat("─", w-2)+"╯"))
	return b.String()
}

func pill(id string, _ color.Color) string {
	return paint(" "+padTo(id, 6)+" ", cBgDim, colorFor(id), true)
}

func join2(a, b string) string { return lipgloss.JoinHorizontal(lipgloss.Top, a, b) }

// ---------------------------------------------------------------- main screen

func (m *model) viewMain() string {
	l := m.layout()
	var out []string
	out = append(out, m.viewHeader()...)
	out = append(out, m.viewInputLine())
	out = append(out, m.viewChips())

	var body string
	switch m.tab {
	case tabProviders:
		body = m.viewProviders(l)
	default:
		list := m.viewList(l)
		if l.detailW > 0 {
			body = join2(list, m.viewDetail(l))
		} else {
			body = list
		}
	}
	out = append(out, body)
	if l.dockH > 0 {
		out = append(out, m.viewDock(l))
	}
	out = append(out, m.viewFooter())
	return strings.Join(out, "\n")
}

func (m *model) tabLabel(t tabID) string {
	n := tabNames[t]
	switch t {
	case tabInstalled:
		if ls := m.lists[t]; ls.loaded {
			n += " " + itoa(len(ls.items))
		}
	case tabUpdates:
		if ls := m.lists[t]; ls.loaded {
			n += " " + itoa(len(ls.items))
		}
	case tabProviders:
		n += " " + itoa(len(m.enabledIDs()))
	}
	return n
}

func (m *model) viewHeader() []string {
	avail := 0
	for _, p := range m.provs {
		if p.Available {
			avail++
		}
	}
	left := " " + fg(cOrange, "=^..^=") + "  " + paint(icon()+" parcels", cGreen, nil, true) + fg(cGrey0, "  ·  every package manager, one box")
	right := ""
	if m.running != nil {
		right = fg(cForest, waveform(m.frame, 8)) + " "
	}
	right += fg(cGrey1, itoa(avail)+"/"+itoa(len(m.provs))+" managers") + " "
	gap := m.w - ansi.StringWidth(left) - ansi.StringWidth(right)
	l1 := left + strings.Repeat(" ", max(1, gap)) + right

	var tabs strings.Builder
	tabs.WriteString(" ")
	for i := range tabNames {
		lab := " " + m.tabLabel(tabID(i)) + " "
		if tabID(i) == m.tab {
			tabs.WriteString(paint(lab, cBgDim, cForest, true))
			// the forest pill needs readable text
		} else {
			tabs.WriteString(fg(cGrey1, lab))
		}
		tabs.WriteString(" ")
	}
	sep := fg(cForest, strings.Repeat("━", m.w))
	return []string{fit(l1, m.w), fit(tabs.String(), m.w), sep}
}

func (m *model) viewInputLine() string {
	switch {
	case m.tab == tabSearch:
		label := fg(cForest, " ❯ ")
		if m.focusIn {
			label = fg(cGreen, " ❯ ")
		}
		return fit(label+m.input.View(), m.w)
	case m.tab == tabProviders:
		return fit(fg(cGrey1, "   space toggles a manager on/off · r re-detects"), m.w)
	default:
		ls := m.list()
		if m.filtering {
			return fit(fg(cGreen, " / ")+m.filter.View(), m.w)
		}
		q := strings.TrimSpace(ls.query)
		if q != "" {
			return fit(fg(cGrey1, " filter: ")+fg(cFg, q)+fg(cGrey0, "   (/ to edit, esc inside to clear)"), m.w)
		}
		return fit(fg(cGrey0, "   / to filter"), m.w)
	}
}

func (m *model) viewChips() string {
	if m.tab == tabProviders {
		return ""
	}
	ls := m.list()
	var parts []string
	for _, p := range m.provs {
		if !p.Available {
			continue
		}
		c := colorFor(p.ID)
		if m.cfg.isDisabled(p.ID) {
			parts = append(parts, fg(cGrey0, p.ID+" off"))
			continue
		}
		st := ls.stat[p.ID]
		switch {
		case st == nil:
			parts = append(parts, fg(cGrey0, p.ID))
		case st.state == "running":
			parts = append(parts, fg(c, p.ID)+" "+fg(cYellow, spin(m.frame)))
		case st.state == "error":
			parts = append(parts, fg(c, p.ID)+" "+fg(cRed, "✗"))
		case st.state == "skipped":
			parts = append(parts, fg(cGrey0, p.ID+" –"))
		default:
			cnt := fg(cGrey1, itoa(st.count))
			if st.count == 0 {
				cnt = fg(cGrey0, "0")
			}
			parts = append(parts, fg(c, p.ID)+" "+cnt)
		}
	}
	return fit(" "+strings.Join(parts, fg(cGrey0, "  ")), m.w)
}

func (m *model) viewList(l layout) string {
	ls := m.list()
	vis := m.visible()
	rows := l.bodyH - 2
	inner := l.listW - 4

	title := tabNames[m.tab]
	right := ""
	if n := len(ls.sel); n > 0 {
		right = itoa(n) + " selected"
	} else if len(vis) > 0 {
		right = itoa(ls.cur+1) + "/" + itoa(len(vis))
	}

	if len(vis) == 0 {
		return box(title, right, m.emptyState(ls, inner, rows), l.listW, l.bodyH, cForest)
	}
	var lines []string
	for i := ls.top; i < len(vis) && i < ls.top+rows; i++ {
		lines = append(lines, m.row(vis[i], i == ls.cur, ls.sel[vis[i].key()], inner, strings.Fields(ls.query)))
	}
	return box(title, right, lines, l.listW, l.bodyH, cForest)
}

func (m *model) row(it Item, cur, sel bool, w int, terms []string) string {
	var bg color.Color
	if cur {
		bg = cBgGreen
	}
	mark := " "
	if cur {
		mark = "▌"
	}
	check := "○"
	ckc := cGrey0
	if sel {
		check, ckc = "◉", cGreen
	}
	showVer := w >= 50
	verW := 11
	flex := w - 1 - 1 - 1 - 1 - 1 - 8 // mark sp check sp … sp pill
	if showVer {
		flex -= verW + 1
	}
	if flex < 8 {
		flex = 8
	}
	name := it.title()
	if ansi.StringWidth(name) > flex {
		name = ansi.Truncate(name, flex, "…")
	}
	nameW := ansi.StringWidth(name)
	desc := ""
	if rem := flex - nameW - 2; rem > 6 && it.Desc != "" {
		desc = ansi.Truncate(it.Desc, rem, "…")
	}
	used := nameW
	var b strings.Builder
	b.WriteString(paint(mark, cForest, bg, true))
	b.WriteString(paint(" ", nil, bg, false))
	b.WriteString(paint(check, ckc, bg, false))
	b.WriteString(paint(" ", nil, bg, false))
	b.WriteString(renderName(name, terms, bg, cur))
	if desc != "" {
		b.WriteString(paint("  "+desc, cGrey0, bg, false))
		used += 2 + ansi.StringWidth(desc)
	}
	b.WriteString(paint(strings.Repeat(" ", max(0, flex-used)), nil, bg, false))
	b.WriteString(paint(" ", nil, bg, false))
	b.WriteString(paint(" "+padTo(it.Provider, 6)+" ", cBgDim, colorFor(it.Provider), true))
	if showVer {
		v := it.Version
		vc := color.Color(cGrey1)
		if it.Latest != "" {
			v = it.Version + "→" + it.Latest
			vc = cYellow
		}
		b.WriteString(paint(" ", nil, bg, false))
		b.WriteString(paint(fit(v, verW), vc, bg, false))
	}
	return b.String()
}

func (m *model) emptyState(ls *listState, w, rows int) []string {
	var lines []string
	center := func(s string, c color.Color) string {
		pad := max(0, (w-ansi.StringWidth(s))/2)
		return strings.Repeat(" ", pad) + fg(c, s)
	}
	switch {
	case ls.loading:
		lines = append(lines, "", center(spin(m.frame)+"  sniffing around…", cYellow))
	case m.tab == tabSearch && !ls.ran:
		lines = append(lines, "")
		for _, f := range foxArt {
			lines = append(lines, center(f, cOrange))
		}
		lines = append(lines, "", center("type a name, press enter, and every manager answers at once", cGrey1), "")
		lines = append(lines, center("space selects · enter installs · tab changes tabs · ? for keys", cGrey0))
	case ls.errmsg != "":
		lines = append(lines, "", center("✗ "+ls.errmsg, cRed))
	case m.tab == tabSearch:
		lines = append(lines, "", center("no parcels match “"+ls.query+"”", cGrey1), center("try a shorter name, or check the chips above for failed managers", cGrey0))
	case m.tab == tabUpdates && strings.TrimSpace(ls.query) == "":
		lines = append(lines, "", center("everything is up to date ✓", cGreen))
	default:
		lines = append(lines, "", center("nothing here", cGrey1))
	}
	return lines
}

// ---------------------------------------------------------------- detail

func (m *model) viewDetail(l layout) string {
	it, ok := m.current()
	inner := l.detailW - 4
	var lines []string
	if !ok {
		lines = []string{"", fg(cGrey0, "select a parcel to see its details")}
		if len(m.provs) > 0 {
			lines = append(lines, "", fg(cGrey0, "managers found on this machine:"))
			n := 0
			for _, p := range m.provs {
				if p.Available && n < 5 {
					lines = append(lines, "  "+fg(colorFor(p.ID), p.Name)+fg(cGrey0, "  "+p.Version))
					n++
				}
			}
		}
		return box("Details", "", lines, l.detailW, l.bodyH, cForest)
	}
	lines = append(lines, paint(it.title(), cFg, nil, true))
	lines = append(lines, pill(it.Provider, nil)+"  "+fg(cGrey1, it.ID))
	lines = append(lines, "")
	switch {
	case it.Latest != "":
		lines = append(lines, fg(cGrey1, "version  ")+fg(cFg, it.Version)+fg(cGrey1, " → ")+fg(cYellow, it.Latest))
	case it.Version != "":
		lines = append(lines, fg(cGrey1, "version  ")+fg(cFg, it.Version))
	}
	if it.Desc != "" {
		lines = append(lines, "")
		for _, w := range wrap(it.Desc, inner) {
			lines = append(lines, fg(cGrey2, w))
		}
	}
	lines = append(lines, "")
	type act struct{ key, name string }
	var acts []act
	switch it.Kind {
	case "result":
		acts = []act{{"install", "install"}}
	case "installed":
		acts = []act{{"upgrade", "upgrade"}, {"uninstall", "uninstall"}}
	case "outdated":
		acts = []act{{"upgrade", "upgrade"}, {"uninstall", "uninstall"}}
	}
	for _, a := range acts {
		c := it.cmdFor(a.key)
		if c == "" {
			continue
		}
		lines = append(lines, fg(cGrey1, "will "+a.name+":"))
		for _, w := range wrap(c, inner-2) {
			lines = append(lines, paint(" "+w+" ", cGreen, cBgDim, false))
		}
		lines = append(lines, "")
	}
	return box("Details", "", lines, l.detailW, l.bodyH, cForest)
}

// ---------------------------------------------------------------- providers

func (m *model) viewProviders(l layout) string {
	inner := l.listW - 4
	var lines []string
	for i, p := range m.provs {
		var bg color.Color
		mark := " "
		if i == m.provCur {
			bg, mark = cBgGreen, "▌"
		}
		sw := fg(cGrey0, "○ n/a")
		switch {
		case !p.Available:
		case m.cfg.isDisabled(p.ID):
			sw = fg(cGrey1, "○ off")
		default:
			sw = fg(cGreen, "● on ")
		}
		ver := p.Version
		blurb := p.Blurb
		if !p.Available {
			ver = "not found"
			blurb = p.Hint
			if blurb == "" {
				blurb = "not found on PATH or in common install dirs"
			}
			blurb = "→ " + blurb
		}
		left := paint(mark, cForest, bg, true) + " " + sw + " " + pill(p.ID, nil) + " " + paint(padTo(p.Name, 13), cFg, bg, i == m.provCur)
		rest := fg(cGrey1, fit(ver, 22)) + " " + fg(cGrey0, blurb)
		row := left + " " + rest
		if i == m.provCur {
			row = left + " " + paint(fit(ver, 22), cGrey1, bg, false) + paint(" "+blurb, cGrey0, bg, false)
			row = fit(row, inner)
			row += paint(strings.Repeat(" ", max(0, inner-ansi.StringWidth(row))), nil, bg, false)
		}
		lines = append(lines, row)
	}
	if !m.provsLoaded {
		lines = append(lines, "", fg(cYellow, " "+spin(m.frame)+" detecting package managers…"))
	}
	return box("Providers", itoa(len(m.enabledIDs()))+" enabled", lines, l.listW, l.bodyH, cForest)
}

// ---------------------------------------------------------------- dock + footer

func (m *model) viewDock(l layout) string {
	done, total := 0, len(m.tasks)
	for _, t := range m.tasks {
		if t.State == "ok" || t.State == "fail" {
			done++
		}
	}
	var cur *Task
	if m.running != nil {
		cur = m.running
	} else if total > 0 {
		cur = m.tasks[total-1]
	}
	inner := m.w - 4
	var lines []string
	head := ""
	if m.running != nil {
		head = fg(cYellow, spin(m.frame)) + " " + paint(cur.label(), cFg, nil, true) + fg(cGrey1, "  "+itoa(done+1)+"/"+itoa(total)) + "  " + fg(cForest, waveform(m.frame, 10))
	} else if cur != nil {
		icon := fg(cGreen, "✓")
		if cur.State == "fail" {
			icon = fg(cRed, "✗")
		}
		head = icon + " " + paint(cur.label(), cFg, nil, true) + fg(cGrey1, "  "+itoa(done)+"/"+itoa(total)+" finished")
	}
	lines = append(lines, head)
	if cur != nil && cur.Cmd != "" {
		lines = append(lines, fg(cGrey0, "$ "+cur.Cmd))
	}
	room := l.dockH - 2 - len(lines)
	for _, ln := range m.logTail(cur, max(1, room)) {
		c := cGrey2
		if ln.err {
			c = cOrange
		}
		if ln.progress {
			c = cAqua
		}
		lines = append(lines, fg(c, ansi.Truncate(ln.text, inner, "…")))
	}
	return box(icon()+" Tasks", "t hides", lines, m.w, l.dockH, cForest)
}

func (m *model) viewFooter() string {
	var hints string
	switch m.tab {
	case tabSearch:
		hints = "↑↓ move · space select · enter install · / search · r redo · tab tabs · ? help"
		if m.focusIn {
			hints = "enter search · ↓ to results · tab switch tab · ctrl+c quit"
		}
	case tabInstalled:
		hints = "space select · u upgrade · x remove · / filter · r reload · e export · ? help"
	case tabUpdates:
		hints = "space select · enter/u upgrade · U upgrade all · x remove · / filter · ? help"
	default:
		hints = "↑↓ move · space on/off · r re-detect · tab tabs · ? help"
	}
	if m.filtering {
		hints = "type to filter · enter keep · esc clear"
	}
	right := ""
	if m.notice != "" {
		right = paint(" "+ansi.Truncate(m.notice, max(12, m.w/2), "…")+" ", cBgDim, cForest, false)
	} else if m.cfg.Mute {
		right = fg(cGrey0, "muted")
	}
	rw := ansi.StringWidth(right)
	left := fg(cGrey0, " "+hints)
	left = ansi.Truncate(left, max(0, m.w-rw-1), "…")
	gap := m.w - ansi.StringWidth(left) - rw
	return left + strings.Repeat(" ", max(0, gap)) + right
}

// ---------------------------------------------------------------- modals

func (m *model) center(content string) string {
	return lipgloss.Place(m.w, m.h, lipgloss.Center, lipgloss.Center, content)
}

func (m *model) viewConfirm() string {
	c := m.confirm
	bc := color.Color(cForest)
	verb := map[string]string{"install": "Install", "upgrade": "Upgrade", "uninstall": "Remove"}[c.action]
	if c.action == "uninstall" {
		bc = cRed
	}
	w := min(m.w-4, 92)
	var lines []string
	lines = append(lines, "")
	for i, it := range c.items {
		if i >= 8 {
			lines = append(lines, fg(cGrey1, "  …and "+itoa(len(c.items)-8)+" more"))
			break
		}
		cmd := it.cmdFor(c.action)
		if cmd == "" {
			cmd = c.action + " " + it.ID
		}
		lines = append(lines, " "+pill(it.Provider, nil)+" "+fg(cGreen, ansi.Truncate(cmd, w-16, "…")))
	}
	lines = append(lines, "", "  "+paint(" y ", cBgDim, cForest, true)+fg(cGrey2, " run")+"    "+paint(" n ", cBgDim, cGrey1, true)+fg(cGrey2, " cancel"))
	title := icon() + " " + verb + " " + plural(len(c.items), "parcel") + "?"
	return m.center(box(title, "", lines, w, len(lines)+2, bc))
}

func (m *model) viewOverlay() string {
	switch m.overlay {
	case "help":
		return m.center(m.helpBox())
	case "konami":
		return m.viewKonami()
	}
	var art []string
	var cap string
	c := color.Color(cOrange)
	switch m.overlay {
	case "fox":
		art = foxArt
		cap = foxQuotes[(m.frame/7)%len(foxQuotes)]
	case "floppy":
		art = floppyArt
		c = cGreen
		cap = "insert disk 2 of 14 to continue"
	case "42":
		art = []string{"  _  _    ____ ", " | || |  |___ \\", " | || |_   __) |", " |__   _| / __/ ", "    |_|  |_____|"}
		c = cGreen
		cap = "the answer. no package manager has the question."
	case "den", "parcels":
		art = []string{"   ┌────────────┐", "  ╱            ╱│", " ┌────────────┐ │", " │  ═══════   │ │", " │  parcels   │╱", " └────────────┘"}
		c = cForest
		cap = "you found the box. it was parcels all along."
	}
	var lines []string
	lines = append(lines, "")
	for _, a := range art {
		lines = append(lines, "  "+fg(c, a))
	}
	lines = append(lines, "", "  "+fg(cGrey2, cap), "", fg(cGrey0, "  any key to put it back"))
	w := 0
	for _, ln := range lines {
		w = max(w, ansi.StringWidth(ln))
	}
	return m.center(box("", "", lines, min(m.w-2, w+6), len(lines)+2, cForest))
}

func (m *model) helpBox() string {
	k := func(a, b string) string { return " " + paint(padTo(a, 12), cGreen, nil, true) + fg(cGrey2, b) }
	lines := []string{
		"",
		k("tab / 1-4", "switch tabs"),
		k("↑↓ jk g G", "move"),
		k("space  a  A", "select · select all · clear"),
		k("enter  i", "install (or upgrade, depending on tab)"),
		k("u  x  U", "upgrade · remove · upgrade everything"),
		k("/  r", "search or filter · refresh"),
		k("e", "export a manifest of what's installed"),
		k("t  m", "show/hide task log · mute sound"),
		k("q  ctrl+c", "quit · force quit"),
		"",
		fg(cGrey0, " psst: some words do things when searched."),
		fg(cGrey0, " any key closes this."),
	}
	return box(icon()+" keys", "parcels", lines, min(m.w-4, 64), len(lines)+2, cForest)
}

// ---------------------------------------------------------------- splash + konami

func (m *model) viewSplash() string {
	var lines []string
	for _, f := range foxArt {
		lines = append(lines, fg(cOrange, f))
	}
	lines = append(lines, "")
	lines = append(lines, paint("  "+icon()+"  p a r c e l s", cGreen, nil, true))
	lines = append(lines, "")
	n := 14
	fill := min(n, m.frame*2)
	if m.provsLoaded {
		fill = n
	}
	bar := fg(cForest, strings.Repeat("▰", fill)) + fg(cBg2, strings.Repeat("▱", n-fill))
	lines = append(lines, "  "+bar)
	lines = append(lines, "  "+fg(cGrey1, splashQuips[(m.frame/4)%len(splashQuips)]+strings.Repeat(".", m.frame%4)))
	return m.center(strings.Join(lines, "\n"))
}

func (m *model) viewKonami() string {
	glyphs := []rune("▐▌▛▜▟▙░▒")
	rows := make([][]rune, m.h)
	for y := 0; y < m.h; y++ {
		r := make([]rune, m.w)
		for x := 0; x < m.w; x++ {
			r[x] = ' '
			if x%5 == 0 {
				h := (x*7 + 3) % 11
				pos := (y*2 + m.frame*(1+x%3) + h*5) % 23
				if pos < 4 {
					r[x] = glyphs[(x+y+pos)%len(glyphs)]
				}
			}
		}
		rows[y] = r
	}
	var b strings.Builder
	for y := 0; y < m.h; y++ {
		c := cForest
		if y%3 == 0 {
			c = cGreen
		}
		b.WriteString(fg(c, string(rows[y])))
		if y < m.h-1 {
			b.WriteString("\n")
		}
	}
	// splice a centered card over the rain
	card := []string{"", "  ↑ ↑ ↓ ↓ ← → ← → B A", ""}
	for _, f := range foxArt {
		card = append(card, "  "+fg(cOrange, f))
	}
	card = append(card, "", "  "+fg(cGreen, "+30 lives. 0 packages. loomer is proud."), "", fg(cGrey0, "  any key"))
	w := 0
	for _, ln := range card {
		w = max(w, ansi.StringWidth(ln))
	}
	panel := box("", "", card, w+6, len(card)+2, cOrange)
	return overlayAt(b.String(), panel, m.w, m.h)
}

// overlayAt draws panel centered over base (both already styled), line by line.
func overlayAt(base, panel string, w, h int) string {
	bl := strings.Split(base, "\n")
	pl := strings.Split(panel, "\n")
	pw := 0
	for _, p := range pl {
		pw = max(pw, ansi.StringWidth(p))
	}
	y0 := max(0, (h-len(pl))/2)
	x0 := max(0, (w-pw)/2)
	for i, p := range pl {
		y := y0 + i
		if y >= len(bl) {
			break
		}
		left := ansi.Truncate(bl[y], x0, "")
		right := ansi.TruncateLeft(bl[y], x0+pw, "")
		bl[y] = left + padTo(p, pw) + right
	}
	return strings.Join(bl, "\n")
}

// renderName draws a package name with fzf-style highlighting of the matched characters.
func renderName(name string, terms []string, bg color.Color, bold bool) string {
	if len(terms) == 0 {
		return paint(name, cFg, bg, bold)
	}
	set := matchSet(terms, name)
	if len(set) == 0 {
		return paint(name, cFg, bg, bold)
	}
	var b strings.Builder
	var run []rune
	on := false
	flush := func() {
		if len(run) == 0 {
			return
		}
		if on {
			b.WriteString(paint(string(run), cGreen, bg, true))
		} else {
			b.WriteString(paint(string(run), cFg, bg, bold))
		}
		run = run[:0]
	}
	for i, r := range []rune(name) {
		if set[i] != on {
			flush()
			on = set[i]
		}
		run = append(run, r)
	}
	flush()
	return b.String()
}
