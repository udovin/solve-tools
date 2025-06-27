package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"

	"github.com/spf13/cobra"
	"github.com/udovin/solve/api"
)

var ContestsCmd = cobra.Command{
	Use: "contests",
}

func init() {
	registerScopeCmd := cobra.Command{
		Use:  "register-scope",
		RunE: wrapMain(registerContestScope),
	}
	registerScopeCmd.Flags().Int64("contest", 0, "Contest ID")
	registerScopeCmd.Flags().Int64("scope", 0, "Scope ID")
	ContestsCmd.AddCommand(&registerScopeCmd)
	downloadSolutionsCmd := cobra.Command{
		Use:  "download-solutions",
		RunE: wrapMain(downloadContestSolutions),
	}
	downloadSolutionsCmd.Flags().Int64("contest", 0, "Contest ID")
	downloadSolutionsCmd.Flags().Int("workers", 4, "Amount of workers")
	downloadSolutionsCmd.Flags().String("path", "", "Path to solutions")
	ContestsCmd.AddCommand(&downloadSolutionsCmd)
	// Add contests command to root.
	RootCmd.AddCommand(&ContestsCmd)
}

func registerContestScope(ctx *Context) error {
	contest, err := ctx.Cmd.Flags().GetInt64("contest")
	if err != nil {
		return err
	}
	if contest == 0 {
		return fmt.Errorf("contest ID is not specified")
	}
	scope, err := ctx.Cmd.Flags().GetInt64("scope")
	if err != nil {
		return err
	}
	if scope == 0 {
		return fmt.Errorf("scope ID is not specified")
	}
	users, err := ctx.Client.ObserveScopeUsers(ctx, scope)
	if err != nil {
		return err
	}
	for _, user := range users.Users {
		form := api.CreateContestParticipantForm{
			AccountID: user.ID,
			Kind:      api.RegularParticipant,
		}
		_, err := ctx.Client.CreateContestParticipant(ctx, contest, form)
		if err != nil {
			return err
		}
	}
	return nil
}

func downloadContestSolutions(ctx *Context) error {
	contest, err := ctx.Cmd.Flags().GetInt64("contest")
	if err != nil {
		return err
	}
	if contest == 0 {
		return fmt.Errorf("contest ID is not specified")
	}
	path, err := ctx.Cmd.Flags().GetString("path")
	if err != nil {
		return err
	}
	if len(path) == 0 {
		path = fmt.Sprintf("contest-%d", contest)
	}
	workers, err := ctx.Cmd.Flags().GetInt("workers")
	if err != nil {
		return err
	}
	if workers <= 0 {
		workers = 4
	}
	waiter := sync.WaitGroup{}
	defer waiter.Wait()
	queue := make(chan api.ContestSolution, workers)
	once := sync.Once{}
	defer once.Do(func() { close(queue) })
	mutex := sync.Mutex{}
	solutionFiles := map[string][]string{}
	for i := 0; i < workers; i++ {
		waiter.Add(1)
		go func() {
			defer waiter.Done()
			for solution := range queue {
				fullSolution, err := ctx.Client.ObserveContestSolution(ctx, contest, solution.ID)
				if err != nil {
					panic(err)
				}
				problemPath := filepath.Join(path, fullSolution.Problem.Problem.Title)
				if err := os.MkdirAll(problemPath, os.ModePerm); err != nil {
					panic(err)
				}
				config := api.CompilerConfig{}
				if err := json.Unmarshal(fullSolution.Solution.Compiler.Config.JSON, &config); err != nil {
					panic(err)
				}
				solutionName := fmt.Sprintf("%d.%s", fullSolution.ID, config.Extensions[0])
				solutionPath := filepath.Join(problemPath, solutionName)
				if err := os.WriteFile(
					solutionPath,
					[]byte(fullSolution.Solution.Content),
					os.ModePerm,
				); err != nil {
					panic(err)
				}
				func() {
					mutex.Lock()
					defer mutex.Unlock()
					solutionFiles[fullSolution.Problem.Problem.Title] = append(
						solutionFiles[fullSolution.Problem.Problem.Title],
						solutionName,
					)
				}()
			}
		}()
	}
	var beginID int64
	for {
		solutions, err := ctx.Client.ObserveContestSolutions(ctx, contest, beginID)
		if err != nil {
			return err
		}
		for _, solution := range solutions.Solutions {
			select {
			case queue <- solution:
			case <-ctx.Done():
				return ctx.Err()
			}
		}
		beginID = solutions.NextBeginID
		if beginID == 0 {
			break
		}
	}
	once.Do(func() { close(queue) })
	waiter.Wait()
	for problem, solutions := range solutionFiles {
		configPath := filepath.Join(path, problem, "input.txt")
		fd, err := os.Create(configPath)
		if err != nil {
			return err
		}
		defer func() { _ = fd.Close() }()
		fmt.Fprintf(fd, "%d\n", len(solutions))
		for _, solution := range solutions {
			fmt.Fprintf(fd, "%s\n", solution)
		}
		if err := fd.Sync(); err != nil {
			return err
		}
	}
	return nil
}
