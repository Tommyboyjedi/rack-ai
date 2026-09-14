import json
import unittest
from cases import CASES, assess

class AssessmentTests(unittest.TestCase):
    def response(self, changes=None):
        value = dict(choices=[dict(message=dict(content=json.dumps(CASES[1].expected)),
                                  finish_reason='stop')],
                     timings=dict(predicted_n=400, predicted_per_second=13,
                                  prompt_n=150, cache_n=0),
                     usage=dict(prompt_tokens=150, completion_tokens=400))
        if changes:
            value.update(changes)
        return value

    def test_checks_expected_answer_and_backend_decode(self):
        result = assess(CASES[1], self.response())
        self.assertTrue(result['quality_check'])
        self.assertTrue(result['speed_pass'])
        self.assertIn('UNAVAILABLE', result['ttft'])

    def test_cached_prompt_and_short_generation_do_not_pass_speed(self):
        result = assess(CASES[1], self.response(dict(timings=dict(
            predicted_n=1, predicted_per_second=500, cache_n=30000))))
        self.assertFalse(result['speed_pass'])

    def test_missing_timing_cannot_pass(self):
        self.assertFalse(assess(CASES[1], self.response(dict(timings={})))['speed_pass'])

    def test_truncation_or_wrong_answer_fails_quality(self):
        value = self.response()
        value['choices'][0]['finish_reason'] = 'length'
        self.assertFalse(assess(CASES[1], value)['quality_check'])
        value['choices'][0] = dict(message=dict(content='{"empty":42}'), finish_reason='stop')
        self.assertFalse(assess(CASES[1], value)['quality_check'])

    def test_ten_is_marginal_not_pass(self):
        result = assess(CASES[1], self.response(dict(timings=dict(
            predicted_n=400, predicted_per_second=10))))
        self.assertFalse(result['speed_pass'])

if __name__ == '__main__':
    unittest.main(verbosity=2)
