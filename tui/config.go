package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
)

type Config struct {
	Disabled []string `json:"disabled"`
	Mute     bool     `json:"mute"`
}

func configPath() string {
	d, err := os.UserConfigDir()
	if err != nil {
		return ""
	}
	return filepath.Join(d, "parcels", "config.json")
}

func loadConfig() Config {
	var c Config
	if p := configPath(); p != "" {
		if b, err := os.ReadFile(p); err == nil {
			_ = json.Unmarshal(b, &c)
		}
	}
	return c
}

func (c Config) save() {
	p := configPath()
	if p == "" {
		return
	}
	sort.Strings(c.Disabled)
	_ = os.MkdirAll(filepath.Dir(p), 0o755)
	if b, err := json.MarshalIndent(c, "", "  "); err == nil {
		_ = os.WriteFile(p, b, 0o644)
	}
}

func (c Config) isDisabled(id string) bool {
	for _, d := range c.Disabled {
		if d == id {
			return true
		}
	}
	return false
}

func (c *Config) toggle(id string) {
	if c.isDisabled(id) {
		var n []string
		for _, d := range c.Disabled {
			if d != id {
				n = append(n, d)
			}
		}
		c.Disabled = n
	} else {
		c.Disabled = append(c.Disabled, id)
	}
}
