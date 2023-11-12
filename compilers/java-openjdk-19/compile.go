package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
)

var classNameReg = regexp.MustCompile(`\s*public\s+class\s+([a-zA-Z0-9_]+)`)

func getClassName(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	matches := classNameReg.FindSubmatch(data)
	if len(matches) < 2 {
		return "Main", nil
	}
	return string(matches[1]), nil
}

func writeManifest(file string, className string) error {
	fd, err := os.Create(file)
	if err != nil {
		return err
	}
	defer func() { _ = fd.Close() }()
	_, err = fd.WriteString(
		"Manifest-Version: 1.0\n" +
			"Main-Class: " + className + "\n",
	)
	if err != nil {
		return err
	}
	return fd.Sync()
}

func runCmd(name string, args ...string) error {
	cmd := exec.Command(name, args...)
	cmd.Stderr = os.Stderr
	cmd.Stdout = os.Stdout
	return cmd.Run()
}

func main() {
	runtime.GOMAXPROCS(1)
	if len(os.Args) < 3 {
		fmt.Fprintf(os.Stderr, "passed too few arguments\n")
		os.Exit(1)
	}
	inputFile := filepath.Clean(os.Args[1])
	outputFile := filepath.Clean(os.Args[2])
	className, err := getClassName(inputFile)
	if err != nil {
		fmt.Fprintf(os.Stderr, "cannot get class name: %v\n", err)
		os.Exit(1)
	}
	dir := filepath.Dir(inputFile)
	javaFile := filepath.Join(dir, className+".java")
	if inputFile != javaFile {
		if err := os.Rename(inputFile, javaFile); err != nil {
			fmt.Fprintf(
				os.Stderr, "cannot rename file %q to %q\n",
				inputFile, javaFile,
			)
			os.Exit(1)
		}
		defer func() { _ = os.Rename(javaFile, inputFile) }()
	}
	manifestFile := filepath.Join(dir, "manifest.txt")
	if err := writeManifest(manifestFile, className); err != nil {
		fmt.Fprintf(os.Stderr, "cannot write manifest: %v\n", err)
		os.Exit(1)
	}
	if err := runCmd("javac", javaFile); err != nil {
		exitCode := 0
		if exitErr, ok := err.(*exec.ExitError); ok {
			exitCode = exitErr.ExitCode()
		}
		if exitCode == 0 {
			fmt.Fprintf(os.Stderr, "cannot run javac: %v\n", err)
			os.Exit(1)
		}
		os.Exit(exitCode)
	}
	files, err := os.ReadDir(dir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "cannot read dir: %v\n", err)
		os.Exit(1)
	}
	buildArgs := []string{"cfm", outputFile, manifestFile}
	for _, file := range files {
		if strings.HasSuffix(file.Name(), ".class") {
			buildArgs = append(buildArgs, file.Name())
		}
	}
	if err := runCmd("jar", buildArgs...); err != nil {
		exitCode := 0
		if exitErr, ok := err.(*exec.ExitError); ok {
			exitCode = exitErr.ExitCode()
		}
		if exitCode == 0 {
			fmt.Fprintf(os.Stderr, "cannot build jar: %v\n", err)
			os.Exit(1)
		}
		os.Exit(exitCode)
	}
}
