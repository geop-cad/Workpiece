"""What the authoring layer writes into a recipe, without building it (no binary needed):
    python3 -m unittest discover -s python/tests
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from workpiece import Part, datum, topo, view  # noqa: E402


class Recipes(unittest.TestCase):
    def drawing_args(self, part, **more):
        part.drawing(views=[view("front", "Front", (100, 100))], **more)
        return [s for s in part.recipe()["steps"] if isinstance(s, dict) and s.get("op") == "Drawing"][-1]["args"]

    def test_hole_counts_are_written_iso_by_default(self):
        # `4× Ø9`, not `(4) PLACES`, unless the sheet asks for it.
        self.assertFalse(self.drawing_args(Part())["places"])
        self.assertTrue(self.drawing_args(Part(), places=True)["places"])

    def test_planes_are_said_one_way(self):
        p = Part()
        p.plane("Z", 5, id="mid")
        s = p.sketch(datum("mid"), id="on_mid")
        f = p.sketch(topo("block/end"), id="on_face")
        plane = {x["sketch"]["id"]: x["sketch"]["plane"] for x in p.recipe()["steps"] if "sketch" in x}
        self.assertEqual(plane[s.id], {"Datum": "mid"})
        self.assertEqual(plane[f.id], {"Topology": "block/end"})

    def test_a_part_can_be_drawn_whole_in_a_section(self):
        self.assertEqual(Part(title="SHAFT", sectioned=False).recipe()["properties"],
                         {"Title": "SHAFT", "Sectioned": "No"})
        self.assertNotIn("properties", Part().recipe())


if __name__ == "__main__":
    unittest.main()
