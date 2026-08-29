import React from 'react';
import { ArchaeologyData } from '../types';
import { Skull, RotateCcw, Shuffle, Sparkles } from 'lucide-react';

interface ArchaeologyPanelProps {
  data: ArchaeologyData;
}

export const ArchaeologyPanel: React.FC<ArchaeologyPanelProps> = ({ data }) => {
  const { most_expensive_task, longest_recovery_loop, model_hopping, weird_discoveries } = data;

  return (
    <section className="py-8 sm:py-12 border-b border-zinc-300 bg-[#f8f9fa]">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        
        {/* Section Title */}
        <div className="flex items-center space-x-2 mb-6">
          <div className="w-3 h-3 bg-black"></div>
          <h2 className="text-xl sm:text-2xl font-mono font-bold uppercase tracking-tight text-black">
            YOUR AGENT ARCHAEOLOGY
          </h2>
          <span className="text-xs font-mono text-zinc-500 hidden sm:inline">
            // strange fossils excavated from ~/.config
          </span>
        </div>

        {/* 3 Main Highlight Cards */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-5 mb-8">
          
          {/* Card 1: Most Expensive Unsolved Task */}
          <div className="bg-[#fdfdfd] border-2 border-zinc-900 p-5 font-mono shadow-[3px_3px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between relative overflow-hidden">
            <div className="absolute top-2 right-2 px-1.5 py-0.5 bg-red-100 border border-red-400 text-red-800 text-[10px] uppercase font-bold flex items-center space-x-1">
              <Skull className="w-3 h-3" />
              <span>UNRESOLVED</span>
            </div>

            <div>
              <div className="text-[11px] text-zinc-500 font-bold uppercase mb-1">
                {most_expensive_task.title}
              </div>
              <div className="text-sm font-extrabold text-black mb-3 p-2 bg-zinc-100 border border-zinc-300 italic">
                &ldquo;{most_expensive_task.prompt}&rdquo;
              </div>

              <div className="space-y-1.5 text-xs text-zinc-700 border-t border-zinc-200 pt-2">
                <div className="flex justify-between">
                  <span className="text-zinc-500">Tokens Burned:</span>
                  <span className="font-bold text-red-600">{most_expensive_task.tokens}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Models Consulted:</span>
                  <span className="font-semibold text-black">{most_expensive_task.models_count} different models</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Time Spent:</span>
                  <span className="font-semibold text-black">{most_expensive_task.duration}</span>
                </div>
              </div>
            </div>

            <div className="mt-4 pt-2 border-t border-dashed border-zinc-300 text-[11px] text-zinc-600 bg-amber-50/70 p-2 border border-amber-200">
              <span className="font-bold text-amber-900">Post-mortem: </span>
              {most_expensive_task.notes}
            </div>
          </div>

          {/* Card 2: Longest Recovery Loop */}
          <div className="bg-[#fdfdfd] border-2 border-zinc-900 p-5 font-mono shadow-[3px_3px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between relative overflow-hidden">
            <div className="absolute top-2 right-2 px-1.5 py-0.5 bg-emerald-100 border border-emerald-500 text-emerald-800 text-[10px] uppercase font-bold flex items-center space-x-1">
              <RotateCcw className="w-3 h-3" />
              <span>RECOVERED</span>
            </div>

            <div>
              <div className="text-[11px] text-zinc-500 font-bold uppercase mb-1">
                {longest_recovery_loop.title}
              </div>
              <div className="text-xs font-semibold text-zinc-900 mb-2 p-2 bg-red-50 border border-red-200 text-red-900 overflow-x-auto">
                <code>{longest_recovery_loop.initial_error}</code>
              </div>

              <div className="space-y-1.5 text-xs text-zinc-700 border-t border-zinc-200 pt-2">
                <div className="flex justify-between">
                  <span className="text-zinc-500">Failed Attempts:</span>
                  <span className="font-bold text-red-600">{longest_recovery_loop.attempts_count} iterations</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Tool Executions:</span>
                  <span className="font-semibold text-black">{longest_recovery_loop.tool_calls} tool calls</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-zinc-500">Tokens Consumed:</span>
                  <span className="font-semibold text-black">{longest_recovery_loop.tokens_burned}</span>
                </div>
              </div>
            </div>

            <div className="mt-4 pt-2 border-t border-dashed border-zinc-300 text-[11px] text-emerald-900 bg-emerald-50/70 p-2 border border-emerald-200">
              <span className="font-bold">Resolution: </span>
              {longest_recovery_loop.final_resolution}
            </div>
          </div>

          {/* Card 3: Model Hopping Ping-Pong */}
          <div className="bg-[#fdfdfd] border-2 border-zinc-900 p-5 font-mono shadow-[3px_3px_0px_0px_rgba(0,0,0,1)] flex flex-col justify-between relative overflow-hidden">
            <div className="absolute top-2 right-2 px-1.5 py-0.5 bg-purple-100 border border-purple-400 text-purple-800 text-[10px] uppercase font-bold flex items-center space-x-1">
              <Shuffle className="w-3 h-3" />
              <span>FALLBACK RELAY</span>
            </div>

            <div>
              <div className="text-[11px] text-zinc-500 font-bold uppercase mb-1">
                {model_hopping.title}
              </div>
              <div className="space-y-1.5 my-2">
                {model_hopping.sequence.map((step, idx) => (
                  <div key={idx} className="flex items-center space-x-2 text-[11px]">
                    <span className="w-4 h-4 rounded-full bg-zinc-200 text-zinc-700 flex items-center justify-center font-bold text-[9px]">
                      {idx + 1}
                    </span>
                    <span className="text-zinc-800">{step}</span>
                  </div>
                ))}
              </div>
            </div>

            <div className="mt-4 pt-2 border-t border-dashed border-zinc-300 text-[11px] text-zinc-600 bg-purple-50/70 p-2 border border-purple-200">
              <div className="flex justify-between font-bold text-purple-900 mb-0.5">
                <span>Orchestrator Cost:</span>
                <span>{model_hopping.total_cost}</span>
              </div>
              <div>{model_hopping.reason}</div>
            </div>
          </div>

        </div>

        {/* Weird Discoveries Grid */}
        <div className="border border-zinc-900 bg-[#fdfdfd] p-5 shadow-[3px_3px_0px_0px_rgba(0,0,0,1)]">
          <div className="flex items-center justify-between mb-4 border-b border-zinc-200 pb-2">
            <div className="flex items-center space-x-2">
              <Sparkles className="w-4 h-4 text-amber-500" />
              <span className="font-mono font-bold text-xs uppercase tracking-wide text-black">
                WEIRD & HILARIOUS ARCHAEOLOGICAL FINDS
              </span>
            </div>
            <span className="text-[11px] font-mono text-zinc-500">
              Extracted via AST & outcome analyzer
            </span>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
            {weird_discoveries.map((disc) => (
              <div
                key={disc.id}
                className="p-3.5 border border-zinc-300 bg-zinc-50/60 font-mono flex flex-col justify-between hover:border-zinc-900 transition-colors"
              >
                <div>
                  <div className="flex justify-between items-start mb-1.5">
                    <span className="font-bold text-xs text-black">{disc.title}</span>
                    <span className="text-[9px] px-1 py-0.5 bg-black text-white font-semibold">
                      {disc.stat}
                    </span>
                  </div>
                  <p className="text-[11px] text-zinc-600 leading-relaxed">
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
