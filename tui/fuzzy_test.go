package main

import (
	"reflect"
	"sort"
	"testing"
)

func TestFuzzyBasics(t *testing.T) {
	if _, _, ok := fuzzy("fzf", "ripgrep", false); ok {
		t.Fatal("fzf must not match ripgrep")
	}
	if _, _, ok := fuzzy("rg", "ripgrep", false); !ok {
		t.Fatal("rg should match ripgrep")
	}
	if _, _, ok := fuzzy("RG", "ripgrep", false); ok {
		t.Fatal("uppercase pattern is case-sensitive")
	}
	if _, _, ok := fuzzy("rg", "RipGrep", false); !ok {
		t.Fatal("lowercase pattern is case-insensitive")
	}
}

func TestFuzzyPrefersBoundariesAndRuns(t *testing.T) {
	a, _, _ := fuzzy("fz", "fzf-lua", false)
	b, _, _ := fuzzy("fz", "buffzone", false)
	if a <= b {
		t.Fatalf("prefix/boundary %d should beat mid-word %d", a, b)
	}
	c, _, _ := fuzzy("gc", "git-commit", false)
	d, _, _ := fuzzy("gc", "agcx", false)
	if c <= d {
		t.Fatalf("boundary hits %d should beat buried hits %d", c, d)
	}
}

func TestFuzzyPositions(t *testing.T) {
	_, pos, ok := fuzzy("fzf", "junegunn/fzf", true)
	if !ok || !reflect.DeepEqual(pos, []int{9, 10, 11}) {
		t.Fatalf("pos=%v ok=%v", pos, ok)
	}
	_, pos, _ = fuzzy("rg", "ripgrep", true)
	if len(pos) != 2 || pos[0] != 0 {
		t.Fatalf("pos=%v", pos)
	}
}

func rank(q string, items []Item) []string {
	for i := range items {
		rankItem(&items[i], q)
	}
	sort.SliceStable(items, func(a, b int) bool {
		x, y := items[a], items[b]
		if x.Tier != y.Tier {
			return x.Tier < y.Tier
		}
		if x.Score != y.Score {
			return x.Score > y.Score
		}
		return provRank(x.Provider) < provRank(y.Provider)
	})
	var out []string
	for _, it := range items {
		out = append(out, it.Provider+":"+it.ID)
	}
	return out
}

func TestExactThenFuzzy(t *testing.T) {
	items := []Item{
		{Provider: "npm", ID: "fzf-lua-extras", Name: "fzf-lua-extras"},
		{Provider: "git", ID: "junegunn/fzf", Name: "junegunn/fzf"},
		{Provider: "winget", ID: "junegunn.fzf", Name: "fzf"},
		{Provider: "cargo", ID: "skim-fzf-compat", Name: "skim-fzf-compat"},
		{Provider: "npm", ID: "left-pad", Name: "left-pad", Desc: "fzf is not involved"},
		{Provider: "scoop", ID: "fzf", Name: "fzf"},
		{Provider: "go", ID: "github.com/junegunn/fzf", Name: "github.com/junegunn/fzf"},
		{Provider: "npm", ID: "fzf", Name: "fzf"},
	}
	got := rank("fzf", items)
	want := []string{"winget:junegunn.fzf", "scoop:fzf", "npm:fzf", "go:github.com/junegunn/fzf", "git:junegunn/fzf"}
	if !reflect.DeepEqual(got[:5], want) {
		t.Fatalf("exact tier wrong:\n got  %v\n want %v", got[:5], want)
	}
	if got[5] != "npm:fzf-lua-extras" {
		t.Fatalf("prefix match should lead the fuzzy tier, got %v", got[5:])
	}
	if got[len(got)-1] != "npm:left-pad" {
		t.Fatalf("description-only hit must be last, got %v", got)
	}
}

func TestMultiWordAnd(t *testing.T) {
	items := []Item{{Provider: "npm", ID: "git-extras"}, {Provider: "npm", ID: "gitx"}}
	got := rank("git extras", items)
	if got[0] != "npm:git-extras" {
		t.Fatalf("got %v", got)
	}
	if items[1].Tier != 2 {
		t.Fatalf("gitx lacks 'extras' so it is not a name match, tier=%d", items[1].Tier)
	}
}

func TestCanonicalBeatsScopedLookalike(t *testing.T) {
	items := []Item{
		{Provider: "npm", ID: "@dgoguerra/fzf", Name: "@dgoguerra/fzf"},
		{Provider: "git", ID: "junegunn/fzf", Name: "junegunn/fzf", Version: "★68.8k"},
		{Provider: "npm", ID: "fzf", Name: "fzf"},
	}
	got := rank("fzf", items)
	want := []string{"npm:fzf", "git:junegunn/fzf", "npm:@dgoguerra/fzf"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v want %v", got, want)
	}
	if s := stars(Item{Desc: "★1.2M  something"}); s != 1.2e6 {
		t.Fatalf("stars=%v", s)
	}
}
