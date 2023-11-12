package main

import (
	"context"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/spf13/cobra"
	"github.com/udovin/solve/api"
)

var RootCmd = cobra.Command{}

type Context struct {
	ctx    context.Context
	Cmd    *cobra.Command
	Args   []string
	Client *api.Client
}

func (c *Context) Deadline() (time.Time, bool) {
	return c.ctx.Deadline()
}

func (c *Context) Done() <-chan struct{} {
	return c.ctx.Done()
}

func (c *Context) Err() error {
	return c.ctx.Err()
}

func (c *Context) Value(key any) any {
	return c.ctx.Value(key)
}

var _ context.Context = (*Context)(nil)

func wrapMain(fn func(*Context) error) func(*cobra.Command, []string) error {
	return func(cmd *cobra.Command, args []string) error {
		ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
		defer cancel()
		endpoint, err := cmd.Flags().GetString("endpoint")
		if err != nil {
			return err
		}
		sessionCookie, err := cmd.Flags().GetString("session-cookie")
		if err != nil {
			return err
		}
		client := api.NewClient(
			endpoint,
			api.WithSessionCookie(sessionCookie),
			api.WithTimeout(10*time.Minute),
		)
		cmdCtx := Context{
			ctx:    ctx,
			Cmd:    cmd,
			Args:   args,
			Client: client,
		}
		return fn(&cmdCtx)
	}
}

func main() {
	RootCmd.Use = os.Args[0]
	RootCmd.PersistentFlags().String("endpoint", "http://localhost:4242/api", "URL to solve API endpoint")
	RootCmd.PersistentFlags().String("session-cookie", "", "Value of session cookie for authorization")
	if err := RootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
