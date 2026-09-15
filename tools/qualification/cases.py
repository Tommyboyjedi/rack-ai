"""Fixed single-model checks, declared before viewing candidate responses."""
from dataclasses import dataclass
import json
import re

MIN_WARM_TOKENS = 256
MIN_DECODE_TPS = 10.0
MARGIN_DECODE_TPS = 12.0

@dataclass(frozen=True)
class Case:
    name: str
    prompt: str
    expected: dict
    warm: bool = False

CASES = (
    Case('reasoning', 'Tasks have unlimited workers: A takes 3 units, B takes 5; '
         'both start at zero. C takes 4 after A. D takes 2 after BOTH A and B. '
         'E takes 6 after BOTH C and D. Give the earliest finish times and makespan. '
         'Start your final answer with a JSON object with keys A,B,C,D,E,makespan. '
         'Then explain the critical paths and why adding workers cannot shorten it.',
         dict(A=3, B=5, C=7, D=7, E=13, makespan=13)),
    Case('warm-coding', 'For a Python function merge_intervals, intervals are closed, '
         'unsorted integer pairs; touching intervals merge. Input is not mutated. '
         'Start your final answer with JSON: empty is result for []; touching is '
         'result for [[5,7],[1,3],[3,5]]; nested is result for [[1,10],[2,3],[12,12]]; '
         'negative is result for [[-3,-1],[-2,2],[4,5]]. Then write the Python '
         'function and explain its invariant, edge cases, complexity and tests in '
         '350-450 words. Do not import libraries or use external tools.',
         dict(empty=[], touching=[[1,7]], nested=[[1,10],[12,12]],
              negative=[[-3,2],[4,5]]), True),
    Case('warm-reasoning', 'A disease has prevalence 1%. A test has sensitivity '
         '90% and false-positive rate 5%. For 10000 people give expected true '
         'positives, false positives and posterior percentage after a positive. '
         'Start your final answer with JSON keys true_positive, false_positive, '
         'posterior_percent (rounded to 2 decimals). Then explain the base-rate '
         'effect, denominators, assumptions and why a second independent test '
         'changes the answer. Write 350-450 words; do not invent independence '
         'for real tests.', dict(true_positive=90, false_positive=495,
         posterior_percent=15.38), True),
    Case('warm-reliability', 'A server durably records request id X as Started, '
         'calls a non-idempotent backend, then loses the response. A retry arrives '
         'with X; a distinct intentional operation arrives with Y. Start your '
         'final answer with JSON keys retry_X (value reconcile), new_Y '
         '(value distinct), and lost_response (value uncertain). Then explain a '
         'safe implementation, durable boundaries, payload conflicts, late '
         'completion and restart handling in 350-450 words. Never claim that '
         'loss of an HTTP connection proves backend computation stopped.',
         dict(retry_X='reconcile', new_Y='distinct', lost_response='uncertain'), True),
)

def assess(case, body):
    choice = body['choices'][0]
    content = choice['message'].get('content') or ''
    match = re.search(r'\{', content)
    try:
        answer = json.JSONDecoder().raw_decode(content[match.start():])[0] if match else None
    except ValueError:
        answer = None
    timings = body.get('timings', {})
    tokens = timings.get('predicted_n')
    rate = timings.get('predicted_per_second')
    quality = (isinstance(answer, dict)
               and all(answer.get(k) == v for k, v in case.expected.items())
               and choice.get('finish_reason') == 'stop')
    sustained = tokens is not None and tokens >= MIN_WARM_TOKENS
    return dict(quality_check=quality, parsed_answer=answer,
                finish_reason=choice.get('finish_reason'), timings=timings,
                usage=body.get('usage'), final_characters=len(content),
                warm=case.warm, sustained_sample=sustained,
                speed_pass=(rate is not None and rate > MIN_DECODE_TPS
                            and sustained) if case.warm else None,
                ttft='UNAVAILABLE: managed response is buffered')
