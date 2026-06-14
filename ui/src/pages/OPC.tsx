import { CardSkeleton } from '@/components/SkeletonLoader'
import { useApp } from '@/context/AppContext'
import OpcAnalyticsDashboard from '@/components/opc/OpcAnalyticsDashboard'
import OPCMissionFocus from '@/components/opc/OPCMissionFocus'
import OPCAgentSwarm from '@/components/opc/OPCAgentSwarm'
import OPCKanbanBoard from '@/components/opc/OPCKanbanBoard'

export default function OPC() {
  const { agents, tasks, config, loading, refreshTasks } = useApp()

  return (
    <div className="flex-1 w-full bg-background overflow-y-auto h-full px-lg py-xl">
      <div className="max-w-[1600px] mx-auto animate-in fade-in duration-700">
        <OPCMissionFocus config={config} />

        {loading ? (
          <div className="grid grid-cols-1 md:grid-cols-3 gap-lg">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        ) : (
          <>
            <OpcAnalyticsDashboard />
            <div className="flex flex-col lg:flex-row gap-lg items-start">
              <OPCAgentSwarm agents={agents} tasks={tasks} />
              <OPCKanbanBoard tasks={tasks} refreshTasks={refreshTasks} />
            </div>
          </>
        )}
      </div>
    </div>
  )
}
