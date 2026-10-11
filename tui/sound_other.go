//go:build !windows

package main

// no speaker api off windows; den is a windows tool. silent here.
func playTones(tones [][2]int) {}
