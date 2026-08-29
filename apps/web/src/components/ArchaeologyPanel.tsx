import React from 'react';
import { ArchaeologyData } from '../types';

interface ArchaeologyPanelProps {
  data: ArchaeologyData;
}

export const ArchaeologyPanel: React.FC<ArchaeologyPanelProps> = ({ data }) => {
  const { most_expensive_task, longest_recovery_loop, model_hopping, weird_discoveries } = data;

  return (
    <section className="py-10 sm:py-14 border-b-2 border-black bg-white">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        
        {/* Section Title */}
        <div className="flex flex-col sm:flex-row sm:items-baseline justify-between gap-2 mb-8 pb-3 border-b-2 border-black">
          <div className="flex items-center space-x-3">
            <div className="w-3 h-3 bg-black"></div>
            <h2 className="text-xl sm:text-2xl font-mono font-extrabold uppercase tracking-tight text-black">
              YOUR AGENT ARCHAEOLOGY
            </h2>
          </div>
          <span className="text-xs font-mono text-zinc-500">
            // strange fossils excavated from ~/.config
          </span>
        </div>

        {/* 3 Main Highlight Cards */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
          
          {/* Card 1: Most Expensive Unsolved Task */}
          <div className="bg-white border-2 border-black p-5 font-mono shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between relative">
            <div className="flex justify-between items-center mb-3">
              <span className="text-[11px] text-zinc-600 font-bold uppercase">
                {most_expensive_task.title}
              </span>
              <span className="px-2 py-0.5 bg-black text-white text-[10px] uppercase font-bold">
                [UNRESOLVED]
              </span>
            </div>

            <div>
              <div className="text-xs font-bold text-black mb-4 p-3 bg-zinc-100 border border-zinc-300">
                &ldquo;{most_expensive_task.prompt}&rdquo;
              </div>

              <div className="space-y-2 text-xs text-zinc-800 border-t border-zinc-200 pt-3">
                <div className="flex justify-between">
                  <span className="text-zinc-500">Tokens Burnt:</span>
                  <span className="font-bold text-black">{most_expensive_task.tokens}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Models Consulted:</span>
                  <span className="font-semibold text-black">{most_expensive_task.models_count} models</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Time Spent:</span>
                  <span className="font-semibold text-black">{most_expensive_task.duration}</span>
                </div>
              </div>
            </div>

            <div className="mt-5 pt-3 border-t-2 border-dashed border-zinc-300 text-[11px] text-zinc-700 bg-zinc-50 p-2.5 border border-zinc-200">
              <span className="font-bold text-black">Post-mortem: </span>
              {most_expensive_task.notes}
            </div>
          </div>

          {/* Card 2: Longest Recovery Loop */}
          <div className="bg-white border-2 border-black p-5 font-mono shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between relative">
            <div className="flex justify-between items-center mb-3">
              <span className="text-[11px] text-zinc-600 font-bold uppercase">
                {longest_recovery_loop.title}
              </span>
              <span className="px-2 py-0.5 bg-black text-white text-[10px] uppercase font-bold">
                [AUTONOMOUS RECOVERY]
              </span>
            </div>

            <div>
              <div className="text-xs font-mono font-medium text-black mb-4 p-3 bg-zinc-100 border border-zinc-300 overflow-x-auto">
                <code>{longest_recovery_loop.initial_error}</code>
              </div>

              <div className="space-y-2 text-xs text-zinc-800 border-t border-zinc-200 pt-3">
                <div className="flex justify-between">
                  <span className="text-zinc-500">Failed Attempts:</span>
                  <span className="font-bold text-black">{longest_recovery_loop.attempts_count} iterations</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Tool Executions:</span>
                  <span className="font-semibold text-black">{longest_recovery_loop.tool_calls} calls</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Tokens Consumed:</span>
                  <span className="font-semibold text-black">{longest_recovery_loop.tokens_burned}</span>
                </div>
              </div>
            </div>

            <div className="mt-5 pt-3 border-t-2 border-dashed border-zinc-300 text-[11px] text-zinc-700 bg-zinc-50 p-2.5 border border-zinc-200">
              <span className="font-bold text-black">Resolution: </span>
              {longest_recovery_loop.final_resolution}
            </div>
          </div>

          {/* Card 3: Model Hopping Ping-Pong */}
          <div className="bg-white border-2 border-black p-5 font-mono shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between relative">
            <div className="flex justify-between items-center mb-3">
              <span className="text-[11px] text-zinc-600 font-bold uppercase">
                {model_hopping.title}
              </span>
              <span className="px-2 py-0.5 bg-black text-white text-[10px] uppercase font-bold">
                [FALLBACK RELAY]
              </span>
            </div>

            <div>
              <div className="space-y-2 my-3">
                {model_hopping.sequence.map((step, idx) => (
                  <div key={idx} className="flex items-center space-x-2 text-xs">
                    <span className="w-5 h-5 bg-black text-white flex items-center justify-center font-bold text-[10px]">
                      {idx + 1}
                    </span>
                    <span className="text-zinc-900 font-medium">{step}</span>
                  </div>
                ))}
              </div>
            </div>

            <div className="mt-5 pt-3 border-t-2 border-dashed border-zinc-300 text-[11px] text-zinc-700 bg-zinc-50 p-2.5 border border-zinc-200">
              <div className="flex justify-between font-bold text-black mb-1">
                <span>Orchestrator Cost:</span>
                <span>{model_hopping.total_cost}</span>
              </div>
              <div>{model_hopping.reason}</div>
            </div>
          </div>

        </div>

        {/* Weird Discoveries Grid */}
        <div className="border-2 border-black bg-white p-6 shadow-[4px_4px_0px_0px_rgba(0,0,0,1)]">
          <div className="flex items-center justify-between mb-5 border-b-2 border-black pb-3">
            <span className="font-mono font-extrabold text-xs uppercase tracking-wider text-black">
              * * * HILARIOUS ARCHAEOLOGICAL FINDS * * *
            </span>
            <span className="text-[11px] font-mono text-zinc-500 hidden sm:inline">
              Extracted via AST & outcome analyzer
            </span>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
            {weird_discoveries.map((disc) => (
              <div
                key={disc.id}
                className="p-4 border border-zinc-900 bg-zinc-50 font-mono flex flex-col justify-between hover:bg-white transition-colors shadow-[2px_2px_0px_0px_rgba(0,0,0,1)]"
              >
                <div>
                  <div className="flex justify-between items-start mb-2">
                    <span className="font-bold text-xs text-black">{disc.title}</span>
                    <span className="text-[10px] px-1.5 py-0.5 bg-black text-white font-bold">
                      {disc.stat}
                    </span>
                  </div>
                  <p className="text-xs text-zinc-600 leading-relaxed">
                    {disc.description}
                  </p>
                </div>
              </div>
            ))}
          </div>
        </div>

      </div>
    </section>
  );
};
