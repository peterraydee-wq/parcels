package main

import (
	"bufio"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"

	tea "charm.land/bubbletea/v2"
)

// Event is one JSON line from den-core. fields are shared across event types.
type Event struct {
	Type     string `json:"type"`
	Provider string `json:"provider"`
	ID       string `json:"id"`
	Name     string `json:"name"`
	Version  string `json:"version"`
	Latest   string `json:"latest"`
	Desc     string `json:"desc"`
	Cmd      string `json:"cmd"`
	Cmd2     string `json:"cmd2"`

	Stream   string `json:"stream"`
	Text     string `json:"text"`
	Progress bool   `json:"progress"`
	OK       bool   `json:"ok"`
	Code     int    `json:"code"`

	Count   int    `json:"count"`
	Ms      int    `json:"ms"`
	Error   string `json:"error"`
	Skipped bool   `json:"skipped"`

	Kind      string   `json:"kind"`
	Blurb     string   `json:"blurb"`
	Available bool     `json:"available"`
	Path      string   `json:"path"`
	Caps      []string `json:"caps"`

	Hint  string   `json:"hint"`
	Via   string   `json:"via"`
	Dirs  []string `json:"dirs"`
	Tried []string `json:"tried"`

	Index  int `json:"index"`
	Total  int `json:"total"`
	Failed int `json:"failed"`
}

func (e *Event) clean() {
	for _, p := range []*string{&e.Provider, &e.ID, &e.Name, &e.Version, &e.Latest, &e.Desc, &e.Cmd, &e.Cmd2, &e.Text, &e.Error, &e.Blurb, &e.Path, &e.Kind, &e.Hint, &e.Via} {
		*p = sanitize(*p)
	}
}

type Core struct{ path string }

func findCore() (*Core, error) {
	exe := "parcels-core"
	if runtime.GOOS == "windows" {
		exe += ".exe"
	}
	if p := os.Getenv("PARCELS_CORE"); p != "" {
		return &Core{p}, nil
	}
	if self, err := os.Executable(); err == nil {
		cand := filepath.Join(filepath.Dir(self), exe)
		if st, err := os.Stat(cand); err == nil && !st.IsDir() {
			return &Core{cand}, nil
		}
	}
	if p, err := exec.LookPath(exe); err == nil {
		return &Core{p}, nil
	}
	return nil, errors.New("parcels-core not found next to parcels, on PATH, or in $PARCELS_CORE")
}

// Job is a running den-core invocation streaming events.
type Job struct {
	ID     int
	Kind   string
	Ch     chan Event
	cancel func()
	task   *Task
	ls     *listState
}

type evMsg struct {
	job    int
	ev     Event
	closed bool
}

func waitEv(j *Job) tea.Cmd {
	return func() tea.Msg {
		ev, ok := <-j.Ch
		if !ok {
			return evMsg{job: j.ID, closed: true}
		}
		return evMsg{job: j.ID, ev: ev}
	}
}

// Start launches den-core and returns a channel of events plus a cancel func.
func (c *Core) Start(args ...string) (chan Event, func(), error) {
	cmd := exec.Command(c.path, args...)
	hideWindow(cmd)
	out, err := cmd.StdoutPipe()
	if err != nil {
		return nil, nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, nil, err
	}
	ch := make(chan Event, 256)
	done := make(chan struct{})
	go func() {
		defer close(ch)
		sc := bufio.NewScanner(out)
		sc.Buffer(make([]byte, 1<<16), 4<<20)
		for sc.Scan() {
			line := strings.TrimSpace(sc.Text())
			if line == "" {
				continue
			}
			var ev Event
			if json.Unmarshal([]byte(line), &ev) != nil {
				continue
			}
			ev.clean()
			select {
			case ch <- ev:
			case <-done:
				_ = cmd.Wait()
				return
			}
		}
		_ = cmd.Wait()
	}()
	cancel := func() {
		select {
		case <-done:
		default:
			close(done)
		}
		if cmd.Process != nil {
			_ = cmd.Process.Kill()
		}
	}
	return ch, cancel, nil
}
