//go:build windows

package main

import "syscall"

// real beeps through kernel32, in a goroutine so the UI never blocks.
var beepProc = syscall.NewLazyDLL("kernel32.dll").NewProc("Beep")

func playTones(tones [][2]int) {
	go func() {
		for _, t := range tones {
			beepProc.Call(uintptr(t[0]), uintptr(t[1]))
		}
	}()
}
