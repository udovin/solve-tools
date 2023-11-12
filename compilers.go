package main

import (
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
	"github.com/udovin/solve/api"
)

var CompilersCmd = cobra.Command{
	Use: "compilers",
}

func init() {
	createCmd := cobra.Command{
		Use:  "create",
		RunE: wrapMain(createCompilersMain),
	}
	createCmd.Flags().Bool("update", false, "Should update existing compilers")
	CompilersCmd.AddCommand(&createCmd)
	// Add compilers command to root.
	RootCmd.AddCommand(&CompilersCmd)
}

type dockerImpl struct {
}

func (d *dockerImpl) BuildImage(ctx context.Context, path string, tag string) error {
	cmd := exec.CommandContext(ctx, "docker", "build", path, "-t", tag)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}

func (d *dockerImpl) CreateContainer(ctx context.Context, tag string) (string, error) {
	cmd := exec.CommandContext(ctx, "docker", "create", tag)
	buf := bytes.Buffer{}
	cmd.Stdout = &buf
	if err := cmd.Run(); err != nil {
		return "", err
	}
	return strings.TrimSpace(buf.String()), nil
}

func (d *dockerImpl) RemoveContainer(ctx context.Context, container string) error {
	cmd := exec.CommandContext(ctx, "docker", "rm", container)
	return cmd.Run()
}

func (d *dockerImpl) ExportRootfs(ctx context.Context, container string, path string) error {
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	defer file.Close()
	writer := gzip.NewWriter(file)
	defer writer.Close()
	cmd := exec.CommandContext(ctx, "docker", "export", container)
	cmd.Stdout = writer
	if err := cmd.Run(); err != nil {
		return err
	}
	return writer.Close()
}

func decodeJSONFile[T any](path string, data *T) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	return json.NewDecoder(file).Decode(data)
}

type CompilerConfig struct {
	api.Compiler
	MapSettings []string `json:"map_settings"`
}

func createCompilersMain(ctx *Context) error {
	update, err := ctx.Cmd.Flags().GetBool("update")
	if err != nil {
		return err
	}
	if err := os.MkdirAll("compiler-images", os.ModePerm); err != nil {
		return err
	}
	files, err := os.ReadDir("compilers")
	if err != nil {
		return err
	}
	compilers, err := ctx.Client.ObserveCompilers(ctx)
	if err != nil {
		return err
	}
	settings, err := ctx.Client.ObserveSettings(ctx)
	if err != nil {
		return err
	}
	byName := map[string]int64{}
	for _, compiler := range compilers.Compilers {
		byName[compiler.Name] = compiler.ID
	}
	byKey := map[string]int64{}
	for _, setting := range settings.Settings {
		byKey[setting.Key] = setting.ID
	}
	docker := dockerImpl{}
	for _, file := range files {
		if !file.IsDir() {
			continue
		}
		compilerPath := filepath.Join("compilers", file.Name())
		configPath := filepath.Join(compilerPath, "config.json")
		imagePath := filepath.Join("compiler-images", file.Name()+".tar.gz")
		config := CompilerConfig{}
		if err := decodeJSONFile(configPath, &config); err != nil {
			return err
		}
		for _, suffix := range config.MapSettings {
			key := "invoker.compilers." + suffix
			if _, ok := byKey[key]; ok {
				continue
			}
			form := api.CreateSettingForm{}
			form.Key = &key
			form.Value = &config.Name
			if _, err := ctx.Client.CreateSetting(ctx, form); err != nil {
				return err
			}
		}
		tag := path.Join("compilers", config.Name)
		if _, ok := byName[config.Name]; ok && !update {
			continue
		}
		if err := func() error {
			exists := true
			if _, err := os.Stat(imagePath); err != nil {
				if !os.IsNotExist(err) {
					return err
				}
				exists = false
			}
			if !exists {
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
				if err := docker.ExportRootfs(ctx, containerID, imagePath); err != nil {
					return err
				}
			}
			println("Upload compiler to API", tag)
			imageFile, err := os.Open(imagePath)
			if err != nil {
				return err
			}
			form := api.UpdateCompilerForm{
				Name:   &config.Name,
				Config: config.Config,
				ImageFile: &api.FileReader{
					Name:   imageFile.Name(),
					Reader: imageFile,
				},
			}
			if id, ok := byName[config.Name]; ok {
				if _, err := ctx.Client.UpdateCompiler(ctx, id, form); err != nil {
					return err
				}
			} else {
				if _, err := ctx.Client.CreateCompiler(
					ctx, api.CreateCompilerForm{UpdateCompilerForm: form},
				); err != nil {
					return err
				}
			}
			println("Compiler uploaded", tag)
			return nil
		}(); err != nil {
			return err
		}
	}
	return nil
}
