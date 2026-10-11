package main

// fzf-style ranking for search results.
//   tier 0  exact name match   (id, name, or the last path segment equals the query)
//   tier 1  fuzzy match        (subsequence match, scored like fzf: boundaries, camelCase,
//                               consecutive runs, prefix/substring bonuses, shorter wins)
//   tier 2  no name match      (the registry returned it anyway, e.g. description hit)
// multiple words are AND-ed, like fzf.

import (
	"math"
	"strconv"
	"strings"
	"unicode"
)

const (
	negInf    = -1 << 30
	sMatch    = 16
	bBoundary = 8
	bCamel    = 7
	bConsec   = 4
	gapCost   = 1
)

func isWordRune(r rune) bool { return unicode.IsLetter(r) || unicode.IsDigit(r) }

// fuzzy scores pat against text. smart-case: an uppercase letter in pat makes it case-sensitive.
// when wantPos is set it also returns the matched rune indices (for highlighting).
func fuzzy(pat, text string, wantPos bool) (int, []int, bool) {
	p, t := []rune(pat), []rune(text)
	m, n := len(p), len(t)
	if m == 0 {
		return 0, nil, true
	}
	if m > n {
		return 0, nil, false
	}
	caseSens := strings.ToLower(pat) != pat
	eq := func(a, b rune) bool {
		if caseSens {
			return a == b
		}
		return unicode.ToLower(a) == unicode.ToLower(b)
	}
	bonus := make([]int, n)
	for j := 0; j < n; j++ {
		switch {
		case j == 0:
			bonus[j] = bBoundary
		case !isWordRune(t[j-1]) && isWordRune(t[j]):
			bonus[j] = bBoundary
		case unicode.IsLower(t[j-1]) && unicode.IsUpper(t[j]):
			bonus[j] = bCamel
		}
	}
	H := make([][]int, m+1)
	M := make([][]int, m+1)
	C := make([][]bool, m+1)
	for i := range H {
		H[i] = make([]int, n+1)
		M[i] = make([]int, n+1)
		C[i] = make([]bool, n+1)
		for j := range M[i] {
			M[i][j] = negInf
			if i > 0 {
				H[i][j] = negInf
			}
		}
	}
	for i := 1; i <= m; i++ {
		for j := 1; j <= n; j++ {
			if eq(p[i-1], t[j-1]) {
				b := bonus[j-1]
				best, cons := negInf, false
				if H[i-1][j-1] > negInf {
					best = H[i-1][j-1] + sMatch + b
				}
				if i > 1 && M[i-1][j-1] > negInf {
					if c := M[i-1][j-1] + sMatch + max(b, bConsec); c > best {
						best, cons = c, true
					}
				}
				M[i][j], C[i][j] = best, cons
			}
			h := M[i][j]
			if H[i][j-1] > negInf && H[i][j-1]-gapCost > h {
				h = H[i][j-1] - gapCost
			}
			H[i][j] = h
		}
	}
	score := H[m][n]
	if score <= negInf {
		return 0, nil, false
	}
	if !wantPos {
		return score, nil, true
	}
	var pos []int
	i, j, inM := m, n, false
	for i > 0 && j > 0 {
		if inM || H[i][j] == M[i][j] {
			pos = append(pos, j-1)
			inM = C[i][j]
			i--
			j--
		} else {
			j--
		}
	}
	for a, b := 0, len(pos)-1; a < b; a, b = a+1, b-1 {
		pos[a], pos[b] = pos[b], pos[a]
	}
	return score, pos, true
}

func lastSegment(id string) string {
	if i := strings.LastIndexAny(id, "./"); i >= 0 && i < len(id)-1 {
		return id[i+1:]
	}
	return id
}

// exactLevel: 2 = whole id or name equals the query, 1 = only the last path segment does
// (scoped/namespaced packages like @someone/fzf), 0 = no exact match.
func exactLevel(it Item, q string) int {
	q = strings.ToLower(strings.TrimSpace(q))
	if q == "" {
		return 0
	}
	if strings.ToLower(it.ID) == q || strings.ToLower(it.Name) == q {
		return 2
	}
	if strings.ToLower(lastSegment(it.ID)) == q {
		return 1
	}
	return 0
}

// stars reads the "★68.8k" the core attaches to GitHub-backed results.
func stars(it Item) float64 {
	for _, s := range []string{it.Version, it.Desc} {
		i := strings.Index(s, "★")
		if i < 0 {
			continue
		}
		r := s[i+len("★"):]
		j := 0
		for j < len(r) && (r[j] >= '0' && r[j] <= '9' || r[j] == '.') {
			j++
		}
		v, err := strconv.ParseFloat(r[:j], 64)
		if err != nil {
			continue
		}
		if j < len(r) {
			switch r[j] {
			case 'k':
				v *= 1e3
			case 'M':
				v *= 1e6
			}
		}
		return v
	}
	return 0
}

// popularity breaks ties toward the canonical project: 0..40, log-scaled stars.
func popularity(it Item) int {
	return min(40, int(math.Log2(stars(it)+1)*2.5))
}

// rankItem fills it.Tier and it.Score for the query.
func rankItem(it *Item, q string) {
	q = strings.TrimSpace(q)
	if q == "" {
		it.Tier, it.Score = 2, 0
		return
	}
	if lvl := exactLevel(*it, q); lvl > 0 {
		it.Tier, it.Score = 0, 50*lvl+popularity(*it)
		return
	}
	cands := []struct {
		s    string
		pen  int
		full bool
	}{{it.Name, 0, true}, {lastSegment(it.ID), 0, true}, {it.ID, 6, true}}
	total := 0
	for _, term := range strings.Fields(q) {
		lt := strings.ToLower(term)
		best := negInf
		for _, c := range cands {
			if c.s == "" {
				continue
			}
			s, _, ok := fuzzy(term, c.s, false)
			if !ok {
				continue
			}
			lc := strings.ToLower(c.s)
			switch {
			case strings.HasPrefix(lc, lt):
				s += 40
			case strings.Contains(lc, lt):
				s += 20
			}
			s -= c.pen + len([]rune(c.s))/3
			if s > best {
				best = s
			}
		}
		if best == negInf {
			// no name match: it only qualifies as a description hit
			it.Tier = 2
			it.Score = 0
			if strings.Contains(strings.ToLower(it.Desc), lt) {
				it.Score = 1
			}
			return
		}
		total += best
	}
	it.Tier, it.Score = 1, total+popularity(*it)/2
}

// matchSet returns the rune indices of text matched by any term (for highlighting).
func matchSet(terms []string, text string) map[int]bool {
	set := map[int]bool{}
	for _, t := range terms {
		if _, pos, ok := fuzzy(t, text, true); ok {
			for _, p := range pos {
				set[p] = true
			}
		}
	}
	return set
}
