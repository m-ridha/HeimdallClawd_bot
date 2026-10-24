"""Railway infrastructure; application releases are uploaded by GitHub Actions."""
from railway_sdk import define_railway, project, service, volume

REGION = "asia-southeast1-eqsg3a"


def deployment(cpu, memory_bytes):
    return {
        "restartPolicyMaxRetries": 3,
        "drainingSeconds": 0,
        "overlapSeconds": 0,
        "limitOverride": {"containers": {"cpu": cpu, "memoryBytes": memory_bytes}},
    }


@define_railway
def main(ctx=None):
    agent_data = volume("heimdall-data", {"region": REGION, "sizeMB": 500})
    # Retain retired storage until its contents are migrated and verified.
    executor_data = volume("executor-workspace", {"region": REGION, "sizeMB": 200})
    # Source is disconnected in Railway; release uploads supply each Docker context.
    # Omit normalized platform defaults (ON_FAILURE and sleeping=false) to avoid drift.
    # The generated agent HTTPS domain targets 3888 and stays platform-managed;
    # Railway IaC's domains field is for custom domains, not *.up.railway.app.
    agent = service(
        "heimdallclawd",
        build={"dockerfilePath": "Dockerfile"},
        healthcheck="/health",
        healthcheckTimeout=180,
        replicas={REGION: 1},
        deploy=deployment(2, 1000000000),
        volumeMounts={"/data": agent_data},
    )
    # Variables stay exclusively on Railway, including future provider credentials.
    return project(
        "HeimdallClawd",
        variables={"managed": False},
        resources=[agent, agent_data, executor_data],
    )
