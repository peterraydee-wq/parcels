package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"

	tea "charm.land/bubbletea/v2"
)

const version = "0.1.0"

func main() {
	core, err := findCore()
	if err != nil {
		fmt.Fprintln(os.Stderr, "parcels:", err)
		os.Exit(1)
	}
	args := os.Args[1:]
	if len(args) > 0 {
		switch args[0] {
		case "-v", "--version", "version":
			fmt.Printf("%s parcels %s  (engine: parcels-core)\n", icon(), version)
			return
		case "doctor":
			os.Exit(runDoctor(core, len(args) > 1 && args[1] == "--dirs"))
		case "-h", "--help", "help":
			usage()
			return
		case "import", "run":
			os.Exit(runPretty(core, args))
		case "providers", "search", "list", "outdated", "export", "fox", "floppy":
			// raw passthrough: JSON lines for scripts, art for the curious
			cmd := exec.Command(core.path, args...)
			cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
			if err := cmd.Run(); err != nil {
				if ee, ok := err.(*exec.ExitError); ok {
					os.Exit(ee.ExitCode())
				}
				os.Exit(1)
			}
			return
		default:
			usage()
			os.Exit(2)
		}
	}
	m := newModel(core)
	if _, err := tea.NewProgram(m).Run(); err != nil {
		fmt.Fprintln(os.Stderr, "parcels:", err)
		os.Exit(1)
	}
}

func usage() {
	fmt.Print(`parcels — every package manager, one box

  parcels                         open the TUI
  parcels search <q>              JSON lines from every manager
  parcels list | outdated         what's installed / what could be newer
  parcels run <install|uninstall|upgrade> <manager> <id>
  parcels export > manifest.json  snapshot everything installed
  parcels import manifest.json    reinstall a snapshot (--pin to match versions)
  parcels providers               which managers were found
  parcels doctor [--dirs]         why a manager was or wasn't found

env: PARCELS_ASCII=1 (no Nerd Font glyphs), PARCELS_CORE (path to parcels-core), PARCELS_REPOS (where git clones go), GITHUB_TOKEN (search rate limits)
`)
}

// runPretty runs `import` / `run` and prints human-friendly progress.
func runPretty(core *Core, args []string) int {
	ch, cancel, err := core.Start(args...)
	if err != nil {
		fmt.Fprintln(os.Stderr, "parcels:", err)
		return 1
	}
	defer cancel()
	g, o, r, d := "\x1b[38;2;167;192;128m", "\x1b[38;2;230;152;117m", "\x1b[38;2;230;126;128m", "\x1b[0m"
	failed := false
	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	for ev := range ch {
		switch ev.Type {
		case "item":
			fmt.Fprintf(w, "%s[%d/%d]%s %s %s\n", g, ev.Index, ev.Total, d, ev.Provider, ev.ID)
		case "plan":
			fmt.Fprintf(w, "  %s$ %s%s\n", g, ev.Cmd, d)
		case "line":
			if ev.Progress {
				continue
			}
			c := ""
			if ev.Stream == "err" {
				c = o
			}
			fmt.Fprintf(w, "  %s%s%s\n", c, strings.TrimSpace(ev.Text), d)
		case "exit":
			if !ev.OK {
				failed = true
				fmt.Fprintf(w, "  %s✗ failed (exit %d)%s\n", r, ev.Code, d)
			} else {
				fmt.Fprintf(w, "  %s✓ ok%s\n", g, d)
			}
		case "done":
			if ev.Error != "" {
				failed = true
				fmt.Fprintf(w, "%s✗ %s%s\n", r, ev.Error, d)
			} else if ev.Total > 0 {
				fmt.Fprintf(w, "\n%s%d parcels, %d failed%s\n", g, ev.Total, ev.Failed, d)
				if ev.Failed > 0 {
					failed = true
				}
			}
		}
		w.Flush()
	}
	if failed {
		return 1
	}
	return 0
}

var _ = json.Marshal

// runDoctor explains detection: which managers were found, where, and how to get the rest.
func runDoctor(core *Core, showDirs bool) int {
	ch, cancel, err := core.Start("doctor")
	if err != nil {
		fmt.Fprintln(os.Stderr, "parcels:", err)
		return 1
	}
	defer cancel()
	g, o, d, dim := "\x1b[38;2;167;192;128m", "\x1b[38;2;230;152;117m", "\x1b[0m", "\x1b[38;2;122;132;120m"
	fmt.Printf("%s%s parcels doctor%s\n\n", g, icon(), d)
	found, total := 0, 0
	var dirs []string
	for ev := range ch {
		switch ev.Type {
		case "dirs":
			dirs = ev.Dirs
		case "doctor":
			total++
			if ev.Available {
				found++
				note := ""
				switch ev.Via {
				case "outside PATH":
					note = o + "  (found outside your PATH — this terminal's PATH may be stale)" + d
				case "python -m pip":
					note = dim + "  (via python -m pip)" + d
				}
				fmt.Printf("  %s✓%s %-7s %s%s%s%s\n", g, d, ev.ID, dim, ev.Path, d, note)
			} else {
				fmt.Printf("  %s✗%s %-7s %snot found → %s%s\n", o, d, ev.ID, dim, ev.Hint, d)
			}
		}
	}
	fmt.Printf("\n%d of %d managers found · searched %d directories\n", found, total, len(dirs))
	if showDirs {
		fmt.Println()
		for _, dir := range dirs {
			fmt.Println("  " + dir)
		}
	} else {
		fmt.Printf("%s(parcels doctor --dirs lists them)%s\n", dim, d)
	}
	return 0
}
