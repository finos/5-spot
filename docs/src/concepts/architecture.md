# Architecture

5-Spot is built as a Kubernetes controller using the kube-rs framework.

## High-Level Architecture

```mermaid
flowchart TB
    subgraph Kubernetes["Kubernetes Cluster"]
        API[Kubernetes API Server]
        
        subgraph CRDs["5-Spot CRDs"]
            SM[ScheduledMachine]
        end
        
        subgraph CAPI["Cluster API Resources"]
            Machine[Machine]
            Bootstrap[Bootstrap Config<br/>e.g., K0sWorkerConfig]
            Infra[Infrastructure<br/>e.g., RemoteMachine]
        end
        
        Node[Node]
    end
    
    subgraph Operator["5-Spot Controller"]
        Controller[Controller Loop]
        Reconciler[Reconciler]
        Scheduler[Schedule Evaluator]
        Creator[Resource Creator]
    end
    
    subgraph External["External"]
        PhysicalMachine[Physical Machine]
    end
    
    Controller -->|watch| API
    API --> SM
    Reconciler --> Scheduler
    Reconciler --> Creator
    Creator -->|create/delete| Bootstrap
    Creator -->|create/delete| Infra
    Creator -->|create/delete| Machine
    Machine --> Node
    Node -.->|joins| PhysicalMachine
    
    SM -->|owns| Bootstrap
    SM -->|owns| Infra
    SM -->|owns| Machine
```

## Component Details

### Controller

The main entry point that:

- Watches for ScheduledMachine resource changes
- Manages reconciliation queue
- Handles multi-instance distribution via consistent hashing
- Provides health and metrics endpoints

### Reconciler

Implements the reconciliation loop:

1. Fetch current ScheduledMachine state
2. Check for kill switch or disabled schedule
3. Evaluate schedule against current time (in configured timezone)
4. Determine desired state (active/inactive)
5. Create or delete CAPI resources as needed
6. Update status, conditions, and references

### Schedule Evaluator

Evaluates time-based schedules:

- Parses cron expressions (when specified)
- Parses day ranges (e.g., `mon-fri`, with wrap-around support)
- Parses hour ranges (e.g., `9-17`, with wrap-around support)
- Handles timezone conversions using IANA timezone database
- Determines if current time is within schedule

### Resource Creator

Creates and manages CAPI resources from inline specs:

- Creates Bootstrap resource from `bootstrapSpec`
- Creates Infrastructure resource from `infrastructureSpec`
- Creates CAPI Machine with references to both
- Sets owner references for automatic garbage collection
- Tracks created resource references in status

## Resource Creation Flow

```mermaid
flowchart TD
    SM[ScheduledMachine] --> |contains| BS[bootstrapSpec<br/>inline config]
    SM --> |contains| IS[infrastructureSpec<br/>inline config]
    
    subgraph Creation["When Schedule Active"]
        BS --> |creates| BR[Bootstrap Resource<br/>e.g., K0sWorkerConfig]
        IS --> |creates| IR[Infrastructure Resource<br/>e.g., RemoteMachine]
        BR --> |referenced by| M[CAPI Machine]
        IR --> |referenced by| M
    end
    
    M --> |provisions| N[Node]
    
    subgraph Status["Status Updated"]
        SM -.-> |bootstrapRef| BR
        SM -.-> |infrastructureRef| IR
        SM -.-> |machineRef| M
        SM -.-> |nodeRef| N
    end
```

## Reconciliation Flow

```mermaid
sequenceDiagram
    participant API as Kubernetes API
    participant Ctrl as Controller
    participant Rec as Reconciler
    participant Sched as Schedule Evaluator
    participant Res as Resource Creator
    
    API->>Ctrl: ScheduledMachine Event
    Ctrl->>Rec: Reconcile Request
    Rec->>API: Get ScheduledMachine
    
    alt killSwitch = true
        Rec->>Res: Delete all resources immediately
        Rec->>API: Update status (Terminated)
    else schedule.enabled = false
        Rec->>API: Update status (Disabled)
    else
        Rec->>Sched: Evaluate Schedule
        Sched-->>Rec: inSchedule: true/false
        
        alt inSchedule = true
            Rec->>Res: Ensure resources exist
            Res->>API: Create Bootstrap from bootstrapSpec
            Res->>API: Create Infrastructure from infrastructureSpec
            Res->>API: Create Machine with refs
            Rec->>API: Update status (Active)
        else inSchedule = false
            Rec->>Res: Remove resources
            Res->>API: Delete Machine
            Res->>API: Delete Bootstrap
            Res->>API: Delete Infrastructure
            Rec->>API: Update status (Inactive)
        end
    end
    
    Rec->>API: Requeue after interval
```

## Multi-Instance Support

5-Spot supports running multiple instances for high availability:

- **Consistent Hashing**: Resources are distributed based on name hash
- **Instance ID**: Each instance has a unique ID (0 to N-1)
- **No Overlap**: Each resource is managed by exactly one instance
- **Environment Variables**: `OPERATOR_INSTANCE_ID` and `OPERATOR_INSTANCE_COUNT`

```mermaid
flowchart LR
    subgraph Resources
        R1[SM: worker-a]
        R2[SM: worker-b]
        R3[SM: worker-c]
        R4[SM: worker-d]
        R5[SM: worker-e]
        R6[SM: worker-f]
    end
    
    subgraph Instances
        I0[Instance 0]
        I1[Instance 1]
        I2[Instance 2]
    end
    
    R1 -->|hash % 3 = 0| I0
    R2 -->|hash % 3 = 1| I1
    R3 -->|hash % 3 = 2| I2
    R4 -->|hash % 3 = 0| I0
    R5 -->|hash % 3 = 1| I1
    R6 -->|hash % 3 = 2| I2
```

## Owner References & Garbage Collection

5-Spot uses Kubernetes owner references for automatic cleanup:

```mermaid
flowchart TD
    SM[ScheduledMachine<br/>owner] 
    SM -->|ownerRef| B[Bootstrap Resource]
    SM -->|ownerRef| I[Infrastructure Resource]
    SM -->|ownerRef| M[CAPI Machine]
    
    subgraph GC["Garbage Collection"]
        D[Delete ScheduledMachine]
        D --> DB[Bootstrap deleted]
        D --> DI[Infrastructure deleted]
        D --> DM[Machine deleted]
    end
```

When a ScheduledMachine is deleted, Kubernetes automatically garbage collects all owned resources.

## Data Flow

```mermaid
flowchart LR
    subgraph Input
        CR[ScheduledMachine CR]
        Time[Current Time]
        TZ[Timezone]
    end
    
    subgraph Processing
        SE[Schedule Evaluation]
        RC[Reconciliation]
    end
    
    subgraph Output
        Resources[CAPI Resources]
        Status[Status Update]
        Events[Kubernetes Events]
        Metrics[Prometheus Metrics]
    end
    
    CR --> SE
    Time --> SE
    TZ --> SE
    SE --> RC
    RC --> Resources
    RC --> Status
    RC --> Events
    RC --> Metrics
```

## Error Handling

| Error Type | Handling | Requeue |
|------------|----------|---------|
| Transient API errors | Automatic retry | 30s with backoff |
| Schedule parse errors | Status updated with error | No requeue |
| Resource creation failures | Retry with backoff | Up to 5m max |
| Permanent errors | Manual intervention required | No automatic retry |

## Observability

### Health Endpoints

- `/health` - Liveness probe (port 8081)
- `/ready` - Readiness probe (port 8081)

### Metrics

- `/metrics` - Prometheus metrics (port 8080)
- Reconciliation duration, success/failure counts
- Resource counts by phase

### Events

Kubernetes events are emitted for:
- Machine creation/deletion
- Schedule activation/deactivation
- Errors and warnings

## Related

- [Concepts Overview](./index.md)
- [Machine Lifecycle](./machine-lifecycle.md)
- [Multi-Instance](../operations/multi-instance.md)
