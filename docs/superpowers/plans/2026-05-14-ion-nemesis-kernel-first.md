# iON NEMESIS Kernel-First Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create `/home/ghost/iON-nemesis` as a fresh GitHub-backed Elixir/OTP NEMESIS kernel repo that runs one real Chronos synthetic intake job end-to-end.

**Architecture:** The new repo is isolated from `/home/ghost/iON`; legacy code is reference-only. The kernel is a supervised Elixir OTP app with a CLI entry point, SQLite-backed state/event/artifact persistence, Oban-backed durable jobs, strict Chronos output validation, reproducibility metadata, and archive/PII guardrails. Phoenix, UI, DuckDB analytics, and real decrypted data are deferred.

**Tech Stack:** Arch Linux, Erlang/OTP, Elixir/Mix, Ecto, `ecto_sqlite3`, Oban, Jason, ExUnit, GNU Make, GitHub CLI.

---

## File Structure

Create a new project at `/home/ghost/iON-nemesis` with this structure:

- `mix.exs`: Mix project, dependencies, escript entry point.
- `config/config.exs`: Ecto/Oban configuration with SQLite database path.
- `lib/ion_nemesis/application.ex`: OTP supervision tree.
- `lib/ion_nemesis/cli.ex`: CLI entry point for `run-chronos`, `run-agents`, and `verify`.
- `lib/ion_nemesis/repo.ex`: Ecto repo.
- `lib/ion_nemesis/nemesis/*.ex`: kernel services for registry, planning, routing, validation, event log, artifact index, reproducibility, failure recovery.
- `lib/ion_nemesis/agents/chronos.ex`: executable Chronos agent.
- `priv/repo/migrations/*.exs`: SQLite schema for cases, jobs, events, artifacts, validation results.
- `priv/schemas/chronos.intake.v1.schema.json`: strict output schema.
- `priv/synthetic/chronos_source/*`: synthetic sample source files only.
- `test/**/*_test.exs`: ExUnit coverage for kernel boot, Chronos job, validation, persistence, and safety scripts.
- `scripts/*.sh`: bootstrap, run, verify, PII scan, sanitize placeholder, pre-archive validation, archive.
- `docs/*.md`: architecture, engine, contracts, build models, reproducibility, migration, and data policy docs.
- `Makefile`: required commands.
- `.gitignore`: excludes build outputs, databases, archives, quarantined data, and generated artifacts.

## Task 1: Tooling And Fresh Repo

**Files:**
- Create directory: `/home/ghost/iON-nemesis`
- Create: `/home/ghost/iON-nemesis/.gitignore`
- Create: `/home/ghost/iON-nemesis/README.md`

- [ ] **Step 1: Verify current tooling state**

Run:

```bash
elixir --version
mix --version
gh auth status
```

Expected before bootstrap on this machine: `elixir` and `mix` are missing; `gh auth status` reports an authenticated `github.com` account.

- [ ] **Step 2: Install Erlang and Elixir on Arch**

Run:

```bash
sudo pacman -S --needed erlang elixir
```

Expected: packages install successfully. If `sudo` requires a password and cannot proceed, stop and report that Elixir/Mix installation is blocked.

- [ ] **Step 3: Verify Elixir and Mix**

Run:

```bash
elixir --version
mix --version
```

Expected: both commands print versions and exit 0.

- [ ] **Step 4: Create fresh project directory and git repo**

Run:

```bash
rm -rf /home/ghost/iON-nemesis
mkdir -p /home/ghost/iON-nemesis
cd /home/ghost/iON-nemesis
git init
```

Expected: a fresh git repository exists at `/home/ghost/iON-nemesis`. This deletes only the new target directory, not `/home/ghost/iON`.

- [ ] **Step 5: Add baseline ignore and readme**

Write `/home/ghost/iON-nemesis/.gitignore`:

```gitignore
/_build/
/deps/
/.elixir_ls/
/cover/
/doc/
/tmp/
/artifacts/
/archives/
/data/
/quarantine/
*.db
*.db-*
*.sqlite
*.sqlite3
*.log
*.tar
*.tar.gz
*.zip
.env
.env.*
```

Write `/home/ghost/iON-nemesis/README.md`:

```markdown
# iON Data Management Systems

NEMESIS ENGINE kernel-first replatform.

Milestone 1 provides a CLI-driven Elixir/OTP kernel that runs the Chronos synthetic intake agent end-to-end with SQLite persistence, Oban durable jobs, strict output validation, iONlog-style events, artifact indexing, and reproducibility metadata.

Real decrypted data is not used in milestone 1.
```

- [ ] **Step 6: Commit tooling baseline**

Run:

```bash
cd /home/ghost/iON-nemesis
git add .gitignore README.md
git commit -m "chore: initialize clean iON NEMESIS repo"
```

Expected: first local working commit exists. The final required commit message will be used after squashing or recreating the final initial commit.

## Task 2: Mix Project And Dependencies

**Files:**
- Create/Modify: `/home/ghost/iON-nemesis/mix.exs`
- Create/Modify: `/home/ghost/iON-nemesis/config/config.exs`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/application.ex`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/repo.ex`
- Test: `/home/ghost/iON-nemesis/test/ion_nemesis/application_test.exs`

- [ ] **Step 1: Generate supervised Mix project in place**

Run:

```bash
cd /home/ghost/iON-nemesis
mix new . --app ion_nemesis --module IonNemesis --sup
```

Expected: Mix asks whether to overwrite `README.md`; answer `n` for README and `Y` for generated Elixir project files if prompted.

- [ ] **Step 2: Configure dependencies and escript**

Replace `/home/ghost/iON-nemesis/mix.exs` with:

```elixir
defmodule IonNemesis.MixProject do
  use Mix.Project

  def project do
    [
      app: :ion_nemesis,
      version: "0.1.0",
      elixir: "~> 1.16",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      escript: [main_module: IonNemesis.CLI]
    ]
  end

  def application do
    [
      extra_applications: [:logger],
      mod: {IonNemesis.Application, []}
    ]
  end

  defp deps do
    [
      {:ecto_sql, "~> 3.12"},
      {:ecto_sqlite3, "~> 0.17"},
      {:oban, "~> 2.18"},
      {:jason, "~> 1.4"}
    ]
  end
end
```

- [ ] **Step 3: Configure SQLite repo and Oban**

Replace `/home/ghost/iON-nemesis/config/config.exs` with:

```elixir
import Config

config :ion_nemesis,
  ecto_repos: [IonNemesis.Repo]

config :ion_nemesis, IonNemesis.Repo,
  database: System.get_env("ION_NEMESIS_DB", "data/nemesis_dev.sqlite3"),
  pool_size: 5,
  stacktrace: true,
  show_sensitive_data_on_connection_error: false

config :ion_nemesis, Oban,
  repo: IonNemesis.Repo,
  plugins: false,
  queues: [chronos: 10]
```

- [ ] **Step 4: Add repo module**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/repo.ex`:

```elixir
defmodule IonNemesis.Repo do
  use Ecto.Repo,
    otp_app: :ion_nemesis,
    adapter: Ecto.Adapters.SQLite3
end
```

- [ ] **Step 5: Add supervised application**

Replace `/home/ghost/iON-nemesis/lib/ion_nemesis/application.ex` with:

```elixir
defmodule IonNemesis.Application do
  use Application

  @impl true
  def start(_type, _args) do
    children = [
      IonNemesis.Repo,
      {Oban, Application.fetch_env!(:ion_nemesis, Oban)}
    ]

    Supervisor.start_link(children, strategy: :one_for_one, name: IonNemesis.Supervisor)
  end
end
```

- [ ] **Step 6: Install dependencies**

Run:

```bash
cd /home/ghost/iON-nemesis
mix local.hex --force
mix local.rebar --force
mix deps.get
```

Expected: dependencies resolve and install.

- [ ] **Step 7: Add application boot test**

Write `/home/ghost/iON-nemesis/test/ion_nemesis/application_test.exs`:

```elixir
defmodule IonNemesis.ApplicationTest do
  use ExUnit.Case

  test "application modules are available" do
    assert Code.ensure_loaded?(IonNemesis.Application)
    assert Code.ensure_loaded?(IonNemesis.Repo)
  end
end
```

- [ ] **Step 8: Run test**

Run:

```bash
cd /home/ghost/iON-nemesis
mix test test/ion_nemesis/application_test.exs
```

Expected: tests pass.

- [ ] **Step 9: Commit Mix foundation**

Run:

```bash
cd /home/ghost/iON-nemesis
git add mix.exs mix.lock config lib test
git commit -m "feat: add Elixir OTP foundation"
```

## Task 3: Persistence Schema

**Files:**
- Create: `/home/ghost/iON-nemesis/priv/repo/migrations/20260514000000_create_nemesis_tables.exs`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/schemas.ex`
- Test: `/home/ghost/iON-nemesis/test/ion_nemesis/nemesis/persistence_test.exs`

- [ ] **Step 1: Add migration**

Write `/home/ghost/iON-nemesis/priv/repo/migrations/20260514000000_create_nemesis_tables.exs`:

```elixir
defmodule IonNemesis.Repo.Migrations.CreateNemesisTables do
  use Ecto.Migration

  def change do
    create table(:cases) do
      add :case_id, :string, null: false
      add :name, :string, null: false
      add :status, :string, null: false
      timestamps(type: :utc_datetime_usec)
    end

    create unique_index(:cases, [:case_id])

    create table(:ion_events) do
      add :case_id, :string, null: false
      add :agent, :string, null: false
      add :event_type, :string, null: false
      add :message, :text, null: false
      add :metadata, :map, null: false, default: %{}
      timestamps(type: :utc_datetime_usec)
    end

    create table(:artifacts) do
      add :case_id, :string, null: false
      add :agent, :string, null: false
      add :artifact_type, :string, null: false
      add :path, :text, null: false
      add :sha256, :string, null: false
      add :metadata, :map, null: false, default: %{}
      timestamps(type: :utc_datetime_usec)
    end

    create table(:validation_results) do
      add :case_id, :string, null: false
      add :agent, :string, null: false
      add :schema_name, :string, null: false
      add :valid, :boolean, null: false
      add :errors, {:array, :string}, null: false, default: []
      timestamps(type: :utc_datetime_usec)
    end
  end
end
```

- [ ] **Step 2: Add Ecto schemas**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/schemas.ex`:

```elixir
defmodule IonNemesis.Nemesis.Schemas.Case do
  use Ecto.Schema
  import Ecto.Changeset

  schema "cases" do
    field :case_id, :string
    field :name, :string
    field :status, :string
    timestamps(type: :utc_datetime_usec)
  end

  def changeset(struct, attrs) do
    struct
    |> cast(attrs, [:case_id, :name, :status])
    |> validate_required([:case_id, :name, :status])
    |> unique_constraint(:case_id)
  end
end

defmodule IonNemesis.Nemesis.Schemas.Event do
  use Ecto.Schema
  import Ecto.Changeset

  schema "ion_events" do
    field :case_id, :string
    field :agent, :string
    field :event_type, :string
    field :message, :string
    field :metadata, :map, default: %{}
    timestamps(type: :utc_datetime_usec)
  end

  def changeset(struct, attrs) do
    struct
    |> cast(attrs, [:case_id, :agent, :event_type, :message, :metadata])
    |> validate_required([:case_id, :agent, :event_type, :message, :metadata])
  end
end

defmodule IonNemesis.Nemesis.Schemas.Artifact do
  use Ecto.Schema
  import Ecto.Changeset

  schema "artifacts" do
    field :case_id, :string
    field :agent, :string
    field :artifact_type, :string
    field :path, :string
    field :sha256, :string
    field :metadata, :map, default: %{}
    timestamps(type: :utc_datetime_usec)
  end

  def changeset(struct, attrs) do
    struct
    |> cast(attrs, [:case_id, :agent, :artifact_type, :path, :sha256, :metadata])
    |> validate_required([:case_id, :agent, :artifact_type, :path, :sha256, :metadata])
  end
end

defmodule IonNemesis.Nemesis.Schemas.ValidationResult do
  use Ecto.Schema
  import Ecto.Changeset

  schema "validation_results" do
    field :case_id, :string
    field :agent, :string
    field :schema_name, :string
    field :valid, :boolean
    field :errors, {:array, :string}, default: []
    timestamps(type: :utc_datetime_usec)
  end

  def changeset(struct, attrs) do
    struct
    |> cast(attrs, [:case_id, :agent, :schema_name, :valid, :errors])
    |> validate_required([:case_id, :agent, :schema_name, :valid, :errors])
  end
end
```

- [ ] **Step 3: Create database**

Run:

```bash
cd /home/ghost/iON-nemesis
mkdir -p data
mix ecto.create
mix ecto.migrate
```

Expected: SQLite database exists at `data/nemesis_dev.sqlite3` and migrations run.

- [ ] **Step 4: Add persistence test**

Write `/home/ghost/iON-nemesis/test/ion_nemesis/nemesis/persistence_test.exs`:

```elixir
defmodule IonNemesis.Nemesis.PersistenceTest do
  use ExUnit.Case

  alias IonNemesis.Nemesis.Schemas.Case
  alias IonNemesis.Repo

  test "case records persist in SQLite" do
    attrs = %{case_id: "case-test-001", name: "Synthetic Case", status: "created"}

    {:ok, record} =
      %Case{}
      |> Case.changeset(attrs)
      |> Repo.insert(on_conflict: :replace_all, conflict_target: :case_id)

    assert record.case_id == "case-test-001"
    assert Repo.get_by!(Case, case_id: "case-test-001").name == "Synthetic Case"
  end
end
```

- [ ] **Step 5: Run persistence test**

Run:

```bash
cd /home/ghost/iON-nemesis
mix test test/ion_nemesis/nemesis/persistence_test.exs
```

Expected: test passes.

- [ ] **Step 6: Commit persistence**

Run:

```bash
cd /home/ghost/iON-nemesis
git add priv lib test
git commit -m "feat: add SQLite persistence schema"
```

## Task 4: NEMESIS Kernel Services

**Files:**
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/agent_registry.ex`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/job_planner.ex`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/task_router.ex`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/event_log.ex`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/artifact_index.ex`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/output_validator.ex`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/repro_manifest.ex`
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/failure_recovery.ex`
- Test: `/home/ghost/iON-nemesis/test/ion_nemesis/nemesis/kernel_services_test.exs`

- [ ] **Step 1: Add agent registry**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/agent_registry.ex`:

```elixir
defmodule IonNemesis.Nemesis.AgentRegistry do
  @chronos %{
    name: "Chronos",
    module: IonNemesis.Agents.Chronos,
    input_schema: "chronos.input.v1",
    output_schema: "chronos.intake.v1",
    queue: :chronos
  }

  def all, do: [@chronos]

  def fetch!("Chronos"), do: @chronos
  def fetch!("chronos"), do: @chronos
end
```

- [ ] **Step 2: Add planner and router**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/job_planner.ex`:

```elixir
defmodule IonNemesis.Nemesis.JobPlanner do
  alias IonNemesis.Nemesis.AgentRegistry

  def chronos_plan(case_id, source_dir, output_dir) do
    agent = AgentRegistry.fetch!("Chronos")

    %{
      agent: agent,
      case_id: case_id,
      input: %{
        "case_id" => case_id,
        "source_dir" => Path.expand(source_dir),
        "output_dir" => Path.expand(output_dir)
      }
    }
  end
end
```

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/task_router.ex`:

```elixir
defmodule IonNemesis.Nemesis.TaskRouter do
  def run(%{agent: %{module: module}, input: input}) do
    module.run(input)
  end
end
```

- [ ] **Step 3: Add event log and artifact index**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/event_log.ex`:

```elixir
defmodule IonNemesis.Nemesis.EventLog do
  alias IonNemesis.Nemesis.Schemas.Event
  alias IonNemesis.Repo

  def record!(attrs) do
    %Event{}
    |> Event.changeset(Map.put_new(attrs, :metadata, %{}))
    |> Repo.insert!()
  end
end
```

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/artifact_index.ex`:

```elixir
defmodule IonNemesis.Nemesis.ArtifactIndex do
  alias IonNemesis.Nemesis.Schemas.Artifact
  alias IonNemesis.Repo

  def record!(attrs) do
    %Artifact{}
    |> Artifact.changeset(Map.put_new(attrs, :metadata, %{}))
    |> Repo.insert!()
  end
end
```

- [ ] **Step 4: Add strict validator**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/output_validator.ex`:

```elixir
defmodule IonNemesis.Nemesis.OutputValidator do
  alias IonNemesis.Nemesis.Schemas.ValidationResult
  alias IonNemesis.Repo

  @required [
    "schema",
    "case_id",
    "agent",
    "source_dir",
    "output_dir",
    "staged_files",
    "events",
    "generated_at"
  ]

  def validate_chronos!(output) do
    errors =
      @required
      |> Enum.reject(&Map.has_key?(output, &1))
      |> Enum.map(&"missing required field: #{&1}")

    errors =
      errors ++
        if output["schema"] == "chronos.intake.v1" do
          []
        else
          ["schema must be chronos.intake.v1"]
        end

    errors =
      errors ++
        if is_list(output["staged_files"]) and length(output["staged_files"]) > 0 do
          []
        else
          ["staged_files must be a non-empty list"]
        end

    valid = errors == []

    %ValidationResult{}
    |> ValidationResult.changeset(%{
      case_id: output["case_id"] || "unknown",
      agent: "Chronos",
      schema_name: "chronos.intake.v1",
      valid: valid,
      errors: errors
    })
    |> Repo.insert!()

    if valid, do: :ok, else: {:error, errors}
  end
end
```

- [ ] **Step 5: Add reproducibility and failure modules**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/repro_manifest.ex`:

```elixir
defmodule IonNemesis.Nemesis.ReproManifest do
  def write!(output_dir, attrs) do
    File.mkdir_p!(output_dir)
    path = Path.join(output_dir, "reproducibility_manifest.json")
    body = Jason.encode!(attrs, pretty: true)
    File.write!(path, body)
    path
  end
end
```

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/nemesis/failure_recovery.ex`:

```elixir
defmodule IonNemesis.Nemesis.FailureRecovery do
  alias IonNemesis.Nemesis.EventLog

  def record!(case_id, agent, reason) do
    EventLog.record!(%{
      case_id: case_id,
      agent: agent,
      event_type: "failure",
      message: inspect(reason),
      metadata: %{"retryable" => true}
    })
  end
end
```

- [ ] **Step 6: Add kernel services test**

Write `/home/ghost/iON-nemesis/test/ion_nemesis/nemesis/kernel_services_test.exs`:

```elixir
defmodule IonNemesis.Nemesis.KernelServicesTest do
  use ExUnit.Case

  alias IonNemesis.Nemesis.AgentRegistry
  alias IonNemesis.Nemesis.JobPlanner

  test "Chronos is registered with strict contracts" do
    agent = AgentRegistry.fetch!("Chronos")

    assert agent.name == "Chronos"
    assert agent.input_schema == "chronos.input.v1"
    assert agent.output_schema == "chronos.intake.v1"
  end

  test "planner creates expanded Chronos input" do
    plan = JobPlanner.chronos_plan("case-001", "priv/synthetic/chronos_source", "artifacts/case-001")

    assert plan.input["case_id"] == "case-001"
    assert String.starts_with?(plan.input["source_dir"], "/")
    assert String.starts_with?(plan.input["output_dir"], "/")
  end
end
```

- [ ] **Step 7: Run tests**

Run:

```bash
cd /home/ghost/iON-nemesis
mix test test/ion_nemesis/nemesis/kernel_services_test.exs
```

Expected: tests pass.

- [ ] **Step 8: Commit kernel services**

Run:

```bash
cd /home/ghost/iON-nemesis
git add lib test
git commit -m "feat: add NEMESIS kernel services"
```

## Task 5: Chronos Synthetic Agent

**Files:**
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/agents/chronos.ex`
- Create: `/home/ghost/iON-nemesis/priv/schemas/chronos.intake.v1.schema.json`
- Create: `/home/ghost/iON-nemesis/priv/synthetic/chronos_source/messages/sample_messages.json`
- Create: `/home/ghost/iON-nemesis/priv/synthetic/chronos_source/media/README.txt`
- Test: `/home/ghost/iON-nemesis/test/ion_nemesis/agents/chronos_test.exs`

- [ ] **Step 1: Add synthetic fixture files**

Write `/home/ghost/iON-nemesis/priv/synthetic/chronos_source/messages/sample_messages.json`:

```json
[
  {
    "id": "synthetic-message-001",
    "from": "synthetic-contact-a",
    "to": "synthetic-contact-b",
    "body": "Synthetic message body for parser validation.",
    "timestamp": "2026-05-14T00:00:00Z"
  }
]
```

Write `/home/ghost/iON-nemesis/priv/synthetic/chronos_source/media/README.txt`:

```text
Synthetic media directory for Chronos intake validation.
No real operator data belongs here.
```

- [ ] **Step 2: Add schema file**

Write `/home/ghost/iON-nemesis/priv/schemas/chronos.intake.v1.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Chronos Intake v1",
  "type": "object",
  "required": ["schema", "case_id", "agent", "source_dir", "output_dir", "staged_files", "events", "generated_at"],
  "additionalProperties": false,
  "properties": {
    "schema": { "const": "chronos.intake.v1" },
    "case_id": { "type": "string", "minLength": 1 },
    "agent": { "const": "Chronos" },
    "source_dir": { "type": "string", "minLength": 1 },
    "output_dir": { "type": "string", "minLength": 1 },
    "staged_files": {
      "type": "array",
      "minItems": 1,
      "items": {
        "type": "object",
        "required": ["relative_path", "staged_path", "sha256", "bytes"],
        "additionalProperties": false,
        "properties": {
          "relative_path": { "type": "string" },
          "staged_path": { "type": "string" },
          "sha256": { "type": "string" },
          "bytes": { "type": "integer", "minimum": 0 }
        }
      }
    },
    "events": { "type": "array", "items": { "type": "string" } },
    "generated_at": { "type": "string" }
  }
}
```

- [ ] **Step 3: Add Chronos implementation**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/agents/chronos.ex`:

```elixir
defmodule IonNemesis.Agents.Chronos do
  alias IonNemesis.Nemesis.ArtifactIndex
  alias IonNemesis.Nemesis.EventLog
  alias IonNemesis.Nemesis.OutputValidator
  alias IonNemesis.Nemesis.ReproManifest
  alias IonNemesis.Nemesis.Schemas.Case
  alias IonNemesis.Repo

  def run(%{"case_id" => case_id, "source_dir" => source_dir, "output_dir" => output_dir}) do
    source_dir = Path.expand(source_dir)
    output_dir = Path.expand(output_dir)
    staged_dir = Path.join(output_dir, "staged")

    with :ok <- ensure_source(source_dir),
         :ok <- File.mkdir_p(staged_dir) do
      upsert_case!(case_id)
      EventLog.record!(event(case_id, "started", "Chronos intake started"))

      staged_files =
        source_dir
        |> files_under()
        |> Enum.map(&stage_file(&1, source_dir, staged_dir, case_id))

      EventLog.record!(event(case_id, "staged", "Chronos staged #{length(staged_files)} files"))

      output = %{
        "schema" => "chronos.intake.v1",
        "case_id" => case_id,
        "agent" => "Chronos",
        "source_dir" => source_dir,
        "output_dir" => output_dir,
        "staged_files" => staged_files,
        "events" => ["started", "staged", "validated", "completed"],
        "generated_at" => DateTime.utc_now() |> DateTime.to_iso8601()
      }

      File.mkdir_p!(output_dir)
      output_path = Path.join(output_dir, "chronos.intake.v1.json")
      File.write!(output_path, Jason.encode!(output, pretty: true))

      case OutputValidator.validate_chronos!(output) do
        :ok ->
          EventLog.record!(event(case_id, "validated", "Chronos output validated"))

          manifest_path =
            ReproManifest.write!(output_dir, %{
              "case_id" => case_id,
              "agent" => "Chronos",
              "command" => "ion_nemesis run-chronos",
              "source_dir" => source_dir,
              "output_path" => output_path,
              "generated_at" => DateTime.utc_now() |> DateTime.to_iso8601(),
              "model_policy" => "No Claude models. Prefer local/free/OpenAI-compatible models. No AI model used in milestone 1 Chronos execution."
            })

          EventLog.record!(event(case_id, "completed", "Chronos intake completed"))
          {:ok, Map.put(output, "manifest_path", manifest_path)}

        {:error, errors} ->
          {:error, errors}
      end
    end
  end

  defp ensure_source(source_dir) do
    if File.dir?(source_dir), do: :ok, else: {:error, "source_dir does not exist: #{source_dir}"}
  end

  defp files_under(source_dir) do
    source_dir
    |> Path.join("**/*")
    |> Path.wildcard()
    |> Enum.filter(&File.regular?/1)
  end

  defp stage_file(path, source_dir, staged_dir, case_id) do
    relative = Path.relative_to(path, source_dir)
    target = Path.join(staged_dir, relative)
    File.mkdir_p!(Path.dirname(target))
    File.cp!(path, target)

    bytes = File.stat!(target).size
    sha256 = sha256_file(target)

    ArtifactIndex.record!(%{
      case_id: case_id,
      agent: "Chronos",
      artifact_type: "staged_file",
      path: target,
      sha256: sha256,
      metadata: %{"relative_path" => relative, "bytes" => bytes}
    })

    %{
      "relative_path" => relative,
      "staged_path" => target,
      "sha256" => sha256,
      "bytes" => bytes
    }
  end

  defp sha256_file(path) do
    path
    |> File.stream!([], 2048)
    |> Enum.reduce(:crypto.hash_init(:sha256), &:crypto.hash_update(&2, &1))
    |> :crypto.hash_final()
    |> Base.encode16(case: :lower)
  end

  defp upsert_case!(case_id) do
    %Case{}
    |> Case.changeset(%{case_id: case_id, name: "Synthetic Chronos Case", status: "intake_started"})
    |> Repo.insert!(on_conflict: {:replace, [:name, :status, :updated_at]}, conflict_target: :case_id)
  end

  defp event(case_id, type, message) do
    %{case_id: case_id, agent: "Chronos", event_type: type, message: message, metadata: %{}}
  end
end
```

- [ ] **Step 4: Add Chronos test**

Write `/home/ghost/iON-nemesis/test/ion_nemesis/agents/chronos_test.exs`:

```elixir
defmodule IonNemesis.Agents.ChronosTest do
  use ExUnit.Case

  alias IonNemesis.Agents.Chronos
  alias IonNemesis.Nemesis.Schemas.{Artifact, Event, ValidationResult}
  alias IonNemesis.Repo

  test "Chronos stages synthetic input and persists audit records" do
    output_dir = Path.expand("tmp/test-artifacts/chronos-case-001")
    File.rm_rf!(output_dir)

    {:ok, output} =
      Chronos.run(%{
        "case_id" => "chronos-case-001",
        "source_dir" => "priv/synthetic/chronos_source",
        "output_dir" => output_dir
      })

    assert output["schema"] == "chronos.intake.v1"
    assert File.exists?(Path.join(output_dir, "chronos.intake.v1.json"))
    assert File.exists?(Path.join(output_dir, "reproducibility_manifest.json"))
    assert length(output["staged_files"]) >= 2
    assert Repo.aggregate(Event, :count) >= 4
    assert Repo.aggregate(Artifact, :count) >= 2
    assert Repo.get_by!(ValidationResult, case_id: "chronos-case-001").valid
  end
end
```

- [ ] **Step 5: Run Chronos test**

Run:

```bash
cd /home/ghost/iON-nemesis
mix test test/ion_nemesis/agents/chronos_test.exs
```

Expected: test passes and writes only synthetic artifacts under `tmp/test-artifacts`.

- [ ] **Step 6: Commit Chronos**

Run:

```bash
cd /home/ghost/iON-nemesis
git add lib priv test
git commit -m "feat: implement Chronos synthetic intake"
```

## Task 6: CLI, Makefile, And Scripts

**Files:**
- Create: `/home/ghost/iON-nemesis/lib/ion_nemesis/cli.ex`
- Create: `/home/ghost/iON-nemesis/Makefile`
- Create: `/home/ghost/iON-nemesis/scripts/bootstrap.sh`
- Create: `/home/ghost/iON-nemesis/scripts/run_all_agents.sh`
- Create: `/home/ghost/iON-nemesis/scripts/verify_outputs.sh`
- Create: `/home/ghost/iON-nemesis/scripts/reproducibility_manifest.sh`
- Create: `/home/ghost/iON-nemesis/scripts/pii_scan.sh`
- Create: `/home/ghost/iON-nemesis/scripts/sanitize_dev_data.sh`
- Create: `/home/ghost/iON-nemesis/scripts/pre_archive_validation.sh`
- Create: `/home/ghost/iON-nemesis/scripts/archive_ion.sh`

- [ ] **Step 1: Add CLI**

Write `/home/ghost/iON-nemesis/lib/ion_nemesis/cli.ex`:

```elixir
defmodule IonNemesis.CLI do
  alias IonNemesis.Nemesis.{JobPlanner, TaskRouter}

  def main(["run-chronos" | args]) do
    opts = parse_args(args)
    case_id = Map.get(opts, "case-id", "synthetic-case-001")
    source = Map.get(opts, "source", "priv/synthetic/chronos_source")
    output = Map.get(opts, "output", "artifacts/#{case_id}")

    plan = JobPlanner.chronos_plan(case_id, source, output)

    case TaskRouter.run(plan) do
      {:ok, result} ->
        IO.puts(Jason.encode!(%{"status" => "ok", "result" => result}, pretty: true))

      {:error, reason} ->
        IO.puts(:stderr, Jason.encode!(%{"status" => "error", "reason" => inspect(reason)}))
        System.halt(1)
    end
  end

  def main(["run-agents"]), do: main(["run-chronos"])
  def main(["verify"]), do: main(["run-chronos", "--case-id", "verify-case-001", "--output", "artifacts/verify-case-001"])

  def main(_args) do
    IO.puts("""
    Usage:
      ion_nemesis run-chronos [--case-id ID] [--source DIR] [--output DIR]
      ion_nemesis run-agents
      ion_nemesis verify
    """)
  end

  defp parse_args(args) do
    args
    |> Enum.chunk_every(2)
    |> Enum.reduce(%{}, fn
      ["--" <> key, value], acc -> Map.put(acc, key, value)
      _, acc -> acc
    end)
  end
end
```

- [ ] **Step 2: Add Makefile**

Write `/home/ghost/iON-nemesis/Makefile`:

```makefile
.PHONY: bootstrap test run run-agents verify archive

bootstrap:
	./scripts/bootstrap.sh

test:
	mix test

run:
	mix run -e 'IonNemesis.CLI.main(["run-chronos"])'

run-agents:
	./scripts/run_all_agents.sh

verify:
	./scripts/verify_outputs.sh

archive:
	./scripts/archive_ion.sh
```

- [ ] **Step 3: Add scripts**

Write each script with executable permissions:

`/home/ghost/iON-nemesis/scripts/bootstrap.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
mkdir -p data artifacts archives tmp
mix deps.get
mix ecto.create
mix ecto.migrate
```

`/home/ghost/iON-nemesis/scripts/run_all_agents.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
mix run -e 'IonNemesis.CLI.main(["run-agents"])'
```

`/home/ghost/iON-nemesis/scripts/verify_outputs.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
./scripts/pii_scan.sh
mix test
mix run -e 'IonNemesis.CLI.main(["verify"])'
test -f artifacts/verify-case-001/chronos.intake.v1.json
test -f artifacts/verify-case-001/reproducibility_manifest.json
./scripts/pre_archive_validation.sh
```

`/home/ghost/iON-nemesis/scripts/reproducibility_manifest.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
mkdir -p artifacts/reproducibility
cat > artifacts/reproducibility/build_manifest.json <<JSON
{
  "project": "iON Data Management Systems",
  "engine": "NEMESIS ENGINE",
  "generated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "git_commit": "$(git rev-parse HEAD 2>/dev/null || echo unknown)",
  "model_policy": "No Claude models. Prefer local/free/OpenAI-compatible models."
}
JSON
```

`/home/ghost/iON-nemesis/scripts/pii_scan.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
scan_paths=(lib config priv/synthetic scripts docs test README.md mix.exs Makefile)
patterns='([A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}|[0-9]{3}-[0-9]{3}-[0-9]{4}|Apple ID|password|token|secret)'
if grep -RInE "$patterns" "${scan_paths[@]}" >/tmp/ion_nemesis_pii_hits 2>/dev/null; then
  cat /tmp/ion_nemesis_pii_hits
  echo "PII scan failed"
  exit 1
fi
echo "PII scan passed"
```

`/home/ghost/iON-nemesis/scripts/sanitize_dev_data.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
echo "Milestone 1 uses synthetic data only; no real development data is sanitized."
```

`/home/ghost/iON-nemesis/scripts/pre_archive_validation.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
test ! -d quarantine
test ! -d data/quarantine
./scripts/pii_scan.sh
echo "Pre-archive validation passed"
```

`/home/ghost/iON-nemesis/scripts/archive_ion.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
./scripts/pre_archive_validation.sh
mkdir -p archives
tar --exclude='./_build' --exclude='./deps' --exclude='./data' --exclude='./artifacts' --exclude='./archives' --exclude='./quarantine' -czf archives/ion-nemesis-source.tar.gz .
echo "Archive written to archives/ion-nemesis-source.tar.gz"
```

- [ ] **Step 4: Make scripts executable**

Run:

```bash
cd /home/ghost/iON-nemesis
chmod +x scripts/*.sh
```

- [ ] **Step 5: Run make targets**

Run:

```bash
cd /home/ghost/iON-nemesis
make bootstrap
make test
make run
make run-agents
make verify
make archive
```

Expected: all targets exit 0 and the archive exists under `archives/`.

- [ ] **Step 6: Commit CLI and scripts**

Run:

```bash
cd /home/ghost/iON-nemesis
git add Makefile lib scripts
git commit -m "feat: add CLI and reproducibility scripts"
```

## Task 7: Documentation

**Files:**
- Create: `/home/ghost/iON-nemesis/docs/ARCHITECTURE.md`
- Create: `/home/ghost/iON-nemesis/docs/NEMESIS_ENGINE.md`
- Create: `/home/ghost/iON-nemesis/docs/AGENT_CONTRACTS.md`
- Create: `/home/ghost/iON-nemesis/docs/BUILD_MODELS.md`
- Create: `/home/ghost/iON-nemesis/docs/REPRODUCIBILITY.md`
- Create: `/home/ghost/iON-nemesis/docs/MIGRATION_FROM_RUST.md`
- Create: `/home/ghost/iON-nemesis/docs/DATA_POLICY.md`
- Create: `/home/ghost/iON-nemesis/docs/PII_REDACTION.md`
- Create: `/home/ghost/iON-nemesis/docs/DEVELOPMENT_DATA_RULES.md`

- [ ] **Step 1: Run model inventory**

Run:

```bash
opencode models > /tmp/ion_nemesis_models.txt
```

Expected: model list is captured. If `opencode models` is unavailable, write that fact in `docs/BUILD_MODELS.md`.

- [ ] **Step 2: Add docs**

Write concise docs with these exact commitments:

`docs/ARCHITECTURE.md`: describe CLI -> JobPlanner -> TaskRouter -> Chronos -> SQLite/EventLog/ArtifactIndex/Validator/ReproManifest.

`docs/NEMESIS_ENGINE.md`: state NEMESIS is an OTP application and list the supervised services.

`docs/AGENT_CONTRACTS.md`: document Chronos input fields and `chronos.intake.v1` output fields.

`docs/BUILD_MODELS.md`: include the output summary from `/tmp/ion_nemesis_models.txt`, state no Claude models are allowed, and state no AI model is used in milestone 1 runtime.

`docs/REPRODUCIBILITY.md`: document all make targets and expected outputs.

`docs/MIGRATION_FROM_RUST.md`: state old Rust/Go/UI trees are reference-only and not copied into milestone 1.

`docs/DATA_POLICY.md`: state real decrypted data is excluded from milestone 1 and must stay quarantined outside git.

`docs/PII_REDACTION.md`: document PII scan patterns and current limitations.

`docs/DEVELOPMENT_DATA_RULES.md`: state synthetic data is the only milestone 1 test fixture source.

- [ ] **Step 3: Verify docs exist**

Run:

```bash
cd /home/ghost/iON-nemesis
test -f docs/ARCHITECTURE.md
test -f docs/NEMESIS_ENGINE.md
test -f docs/AGENT_CONTRACTS.md
test -f docs/BUILD_MODELS.md
test -f docs/REPRODUCIBILITY.md
test -f docs/MIGRATION_FROM_RUST.md
test -f docs/DATA_POLICY.md
test -f docs/PII_REDACTION.md
test -f docs/DEVELOPMENT_DATA_RULES.md
```

Expected: all checks pass.

- [ ] **Step 4: Commit docs**

Run:

```bash
cd /home/ghost/iON-nemesis
git add docs
git commit -m "docs: document NEMESIS kernel milestone"
```

## Task 8: Final Verification, Initial Commit Rewrite, And GitHub Push

**Files:**
- Modify only via git history operations in `/home/ghost/iON-nemesis`

- [ ] **Step 1: Run full verification**

Run:

```bash
cd /home/ghost/iON-nemesis
make bootstrap
make test
make run
make run-agents
make verify
make archive
git status --short
```

Expected: all make targets pass. `git status --short` shows only ignored/generated files or nothing to commit.

- [ ] **Step 2: Create required single initial commit**

Run:

```bash
cd /home/ghost/iON-nemesis
git reset --soft "$(git rev-list --max-parents=0 HEAD)"
git commit --amend -m "Initial iON Data Management Systems NEMESIS replatform"
```

Expected: repository history contains one root commit with the required message.

- [ ] **Step 3: Confirm final commit**

Run:

```bash
cd /home/ghost/iON-nemesis
git log --oneline --max-count=5
git status --short
```

Expected: one commit is visible, message is `Initial iON Data Management Systems NEMESIS replatform`, and no source/doc changes are unstaged.

- [ ] **Step 4: Create GitHub repo and push**

Run:

```bash
cd /home/ghost/iON-nemesis
gh repo create iON-nemesis --private --source=. --remote=origin --push
```

Expected: GitHub creates the repo under the authenticated account and pushes the initial commit. If the repo name already exists, run:

```bash
gh repo create iON-nemesis-kernel --private --source=. --remote=origin --push
```

- [ ] **Step 5: Report final state**

Run:

```bash
cd /home/ghost/iON-nemesis
git remote -v
git log --oneline -1
```

Expected: output shows the GitHub remote and required initial commit.

## Self-Review

Spec coverage:

- Fresh repo isolation is covered in Task 1.
- Elixir/OTP kernel foundation is covered in Task 2.
- Oban + SQLite persistence is covered in Tasks 2 and 3.
- NEMESIS service boundaries are covered in Task 4.
- Chronos synthetic vertical slice is covered in Task 5.
- Strict output validation is covered in Tasks 4 and 5.
- Required make targets and scripts are covered in Task 6.
- Required docs and model policy are covered in Task 7.
- Final local commit, GitHub repo creation, and push are covered in Task 8.

Placeholder scan:

- No open implementation placeholders are intentionally left in the plan.

Scope check:

- The plan does not mutate `/home/ghost/iON` except for this plan file.
- The plan does not import real decrypted backup data.
- Phoenix, UI, DuckDB analytics, and all-agent completion remain out of scope for milestone 1.
