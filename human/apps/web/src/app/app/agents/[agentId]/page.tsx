import { AgentDetailScreen } from "../../../../journeys/agents";

export const dynamic = "force-dynamic";

export default async function AgentDetailPage({
  params,
}: Readonly<{ params: Promise<{ agentId: string }> }>) {
  const { agentId } = await params;
  return (
    <AgentDetailScreen
      agentId={agentId}
    />
  );
}
