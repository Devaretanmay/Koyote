import os
import shutil
import tempfile
import unittest
from unittest.mock import MagicMock

from koyote.ai_planner import AIPatchPlanner, parse_confidence, parse_search_replace_blocks
from koyote.llm import LLMClient, LLMResponse


class TestAIPatchPlanner(unittest.TestCase):
    def setUp(self):
        self.test_dir = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.test_dir, ignore_errors=True)

    def test_parse_search_replace_blocks(self):
        raw_text = (
            "Here is the patch:\n\n"
            "<<<<<<< SEARCH\n"
            "const client = new OldClient();\n"
            "=======\n"
            "const client = new NewClient();\n"
            ">>>>>>> REPLACE\n\n"
            "End of patch."
        )
        blocks = parse_search_replace_blocks(raw_text)
        self.assertEqual(len(blocks), 1)
        self.assertEqual(blocks[0][0], "const client = new OldClient();")
        self.assertEqual(blocks[0][1], "const client = new NewClient();")

    def test_plan_and_apply_mock_llm(self):
        sample_file = os.path.join(self.test_dir, "client.ts")
        with open(sample_file, "w") as f:
            f.write("import Stripe from 'stripe';\nconst stripe = new Stripe('key', { apiVersion: '2020-08-27' });\n")

        mock_client = MagicMock(spec=LLMClient)
        mock_client.complete.return_value = LLMResponse(
            content=(
                "<<<<<<< SEARCH\n"
                "const stripe = new Stripe('key', { apiVersion: '2020-08-27' });\n"
                "=======\n"
                "const stripe = new Stripe('key', { apiVersion: '2024-06-20' });\n"
                ">>>>>>> REPLACE"
            ),
            model="claude-3-5-sonnet",
        )

        planner = AIPatchPlanner(client=mock_client)
        results = planner.plan_and_apply(
            repo_dir=self.test_dir,
            affected_files=["client.ts"],
            provider_name="stripe",
            from_version="11.0.0",
            to_version="13.0.0",
            migration_details="apiVersion change",
            dry_run=False,
        )

        self.assertEqual(len(results), 1)
        self.assertTrue(results[0].success)
        self.assertGreater(results[0].lines_changed, 0)
        self.assertIn("2024-06-20", results[0].unified_diff)

        with open(sample_file, "r") as f:
            updated = f.read()
        self.assertIn("2024-06-20", updated)

    def test_plan_and_apply_with_test_error_self_repair(self):
        sample_file = os.path.join(self.test_dir, "handler.ts")
        with open(sample_file, "w") as f:
            f.write("export function handle(req) { return req.body; }\n")

        mock_client = MagicMock(spec=LLMClient)
        mock_client.complete.return_value = LLMResponse(
            content=(
                "<<<<<<< SEARCH\n"
                "export function handle(req) { return req.body; }\n"
                "=======\n"
                "export function handle(req: any) { return req.body; }\n"
                ">>>>>>> REPLACE"
            ),
            model="gpt-4o",
        )

        planner = AIPatchPlanner(client=mock_client)
        results = planner.plan_and_apply(
            repo_dir=self.test_dir,
            affected_files=["handler.ts"],
            provider_name="custom",
            from_version="1.0.0",
            to_version="2.0.0",
            test_error="TypeError: Parameter 'req' implicitly has an 'any' type.",
            dry_run=False,
        )

        self.assertEqual(len(results), 1)
        prompt_arg = mock_client.complete.call_args[1]["messages"][0]["content"]
        self.assertIn("Parameter 'req' implicitly has an 'any' type.", prompt_arg)


if __name__ == "__main__":
    unittest.main()


def test_parse_confidence_variants(tmp_path=None):
    assert parse_confidence("analysis\nConfidence: high") == "high"
    assert parse_confidence("analysis\n**Confidence:** high") == "high"
    assert parse_confidence("analysis\n**Confidence: medium**") == "medium"
    assert parse_confidence("analysis\nconfidence : low") == "low"
    assert parse_confidence("no confidence line here") == "unknown"
