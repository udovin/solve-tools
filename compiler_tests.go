package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"path"
	"path/filepath"
	"syscall"

	"github.com/spf13/cobra"
)

func init() {
	testCmd := cobra.Command{
		Use:  "test [compiler-name]",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
			defer cancel()
			reset, err := cmd.Flags().GetBool("reset")
			if err != nil {
				return err
			}
			safeexecPath, err := cmd.Flags().GetString("safeexec")
			if err != nil {
				return err
			}
			compilerTestPath, err := cmd.Flags().GetString("compiler-test")
			if err != nil {
				return err
			}
			cgroupName, err := cmd.Flags().GetString("cgroup")
			if err != nil {
				return err
			}
			var filter string
			if len(args) > 0 {
				filter = args[0]
			}
			return testCompilersMain(ctx, safeexecPath, compilerTestPath, cgroupName, filter, reset)
		},
	}
	testCmd.Flags().Bool("reset", false, "Reset canonical test files with actual output")
	testCmd.Flags().String("safeexec", "../solve/cmd/safeexec/safeexec", "Path to safeexec binary")
	testCmd.Flags().String("compiler-test", "../solve/cmd/compiler-test/compiler-test", "Path to compiler-test binary")
	testCmd.Flags().String("cgroup", "../solve-compiler-test", "Cgroup name for safeexec")
	CompilersCmd.AddCommand(&testCmd)
}

func ensureImage(ctx context.Context, docker *dockerImpl, compilerPath, imagePath, tag string) error {
	if _, err := os.Stat(imagePath); err == nil {
		return nil
	}
	if err := os.MkdirAll("compiler-images", os.ModePerm); err != nil {
		return err
	}
	println("Build compiler image", tag)
	if err := docker.BuildImage(ctx, compilerPath, tag); err != nil {
		return err
	}
	println("Create compiler container", tag)
	containerID, err := docker.CreateContainer(ctx, tag)
	if err != nil {
		return err
	}
	defer func() {
		println("Remove compiler container", tag)
		if err := docker.RemoveContainer(ctx, containerID); err != nil {
			println("Error:", err.Error())
		}
	}()
	println("Save compiler rootfs", tag)
	return docker.ExportRootfs(ctx, containerID, imagePath)
}

func testCompilersMain(ctx context.Context, safeexecPath, compilerTestPath, cgroupName, filter string, reset bool) error {
	files, err := os.ReadDir("compilers")
	if err != nil {
		return err
	}
	docker := dockerImpl{}
	hasFailure := false
	for _, file := range files {
		if !file.IsDir() {
			continue
		}
		name := file.Name()
		if filter != "" && name != filter {
			continue
		}
		compilerPath := filepath.Join("compilers", name)
		testsDir := filepath.Join(compilerPath, "tests")
		if _, err := os.Stat(testsDir); err != nil {
			continue
		}
		// Load config to get compiler name for Docker tag.
		config := CompilerConfig{}
		if err := decodeJSONFile(filepath.Join(compilerPath, "config.json"), &config); err != nil {
			return fmt.Errorf("%s: %w", name, err)
		}
		imagePath := filepath.Join("compiler-images", name+".tar.gz")
		tag := path.Join("compilers", config.Name)
		if err := ensureImage(ctx, &docker, compilerPath, imagePath, tag); err != nil {
			return fmt.Errorf("%s: cannot prepare image: %w", name, err)
		}
		// Invoke compiler-test binary.
		args := []string{
			"--safeexec", safeexecPath,
			"--cgroup", cgroupName,
			"--compiler-dir", compilerPath,
			"--image", imagePath,
		}
		if reset {
			args = append(args, "--reset")
		}
		cmd := exec.CommandContext(ctx, compilerTestPath, args...)
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		if err := cmd.Run(); err != nil {
			hasFailure = true
		}
	}
	if hasFailure {
		return fmt.Errorf("some compiler tests failed")
	}
	return nil
}
