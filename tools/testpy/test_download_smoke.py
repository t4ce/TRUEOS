#!/usr/bin/env python3
import copy
import json
import unittest
import download_smoke as smoke


def log(run="new", status=200, route="general", redirect=False):
    url = "https://example.test/a.png"
    start = {"schema":"download_smoke_app_v1","event":"start","run":run,"requests":1,"urls":[url],"routes":[route]}
    base = {"schema":"download_smoke_app_v1","run":run,"case":0,"url":url,"route":route}
    response = {"schema":"download_http_v1","event":"response","url":url,"status":status,"user_agent":"TRUEOS", "body_sha256":"a"*64,"body_bytes":50,"headers":{},"body_preview":"denied" if status >= 400 else None}
    result = dict(base,event="request",completed=True,api_success=status==200 and route!="image",code=0 if status==200 and route!="image" else -8 if status==200 else -6,bytes=50,sha256="a"*64)
    records = [("download-smoke-app",start),("download-smoke-app",dict(base,event="begin"))]
    if redirect:
        records.append(("download-http",dict(response,status=301,headers={"location":"https://next.test/a.png"})))
        response["url"]="https://next.test/a.png"
    records += [("download-http",response),("download-smoke-app",result),("download-smoke-app",dict(schema="download_smoke_app_v1",event="summary",run=run,cases=1,completed=1))]
    return records


def report(events):
    records, issues = smoke.parse_records("\n".join(f"[log] {source}: {json.dumps(record)}" for source,record in events))
    return smoke.summarize_run(records,parse_issues=issues)


def recovered_retry(attempts=1):
    events=log(route="image")
    events[-2][1].update(api_success=True,code=0)
    response=copy.deepcopy(events[2])
    denies=[]
    for index in range(attempts):
        denial=copy.deepcopy(response)
        denial[1].update(status=429,body_preview="rate limited",headers={"retry_after":"1"})
        marker=("download-http",dict(schema="download_http_v1",event="retry",url=response[1]["url"],scope="image",status=429,
                                    attempt=index+1,max_retries=2,delay_ms=1000,retry_after_ms=1000,
                                    elapsed_since_response_ms=0,
                                    elapsed_ms=100+1100*index,remaining_ms=14900-1100*index,budget_ms=15000))
        denies += [denial,marker]
    events[2:2]=denies
    return events


class DownloadSmokeTests(unittest.TestCase):
    def test_retry_after_includes_time_already_elapsed_since_response(self):
        events=recovered_retry();events[3][1].update(elapsed_since_response_ms=2,delay_ms=998)
        self.assertTrue(report(events)["diagnostic_complete"])
        events[3][1]["delay_ms"]=997
        self.assertFalse(report(events)["diagnostic_complete"])
        for elapsed in (-1,101,None):
            events=recovered_retry();events[3][1]["elapsed_since_response_ms"]=elapsed
            self.assertFalse(report(events)["diagnostic_complete"])

    def test_retry_ordinals_and_budget_span_the_full_redirect_chain(self):
        events=recovered_retry(attempts=2)
        redirect=copy.deepcopy(events[2])
        redirect[1].update(status=302,headers={"location":"https://next.test/a.png"})
        for index in (4,5,6): events[index][1]["url"]="https://next.test/a.png"
        events.insert(4,redirect)
        result=report(events)
        self.assertTrue(result["diagnostic_complete"])
        self.assertEqual(result["recovered_http_failures"],2)
        self.assertEqual([r["status"] for r in result["cases"][0]["responses"]],[429,302,429,200])
        events[6][1]["attempt"]=1
        self.assertFalse(report(events)["diagnostic_complete"])

    def test_429_retry_then_200_preserves_failure_and_recovers(self):
        result=report(recovered_retry())
        self.assertTrue(result["diagnostic_complete"])
        self.assertTrue(result["http_success"])
        self.assertEqual(result["http_failures"],0)
        self.assertEqual(result["recovered_http_failures"],1)
        self.assertEqual(result["http_failure_responses"],1)
        self.assertEqual([r["status"] for r in result["cases"][0]["responses"]],[429,200])
        self.assertTrue(report(recovered_retry(attempts=2))["diagnostic_complete"])

    def test_multiple_responses_without_matching_ordered_retry_are_ambiguous(self):
        events=recovered_retry()
        self.assertFalse(report([event for event in events if event[1]["event"]!="retry"])["diagnostic_complete"])
        events[3][1]["url"]="https://other.test/a.png"
        self.assertFalse(report(events)["diagnostic_complete"])
        events=recovered_retry();events[2],events[3]=events[3],events[2]
        self.assertFalse(report(events)["diagnostic_complete"])

    def test_retry_budget_wait_and_limit_are_enforced(self):
        for change in ({"budget_ms":16000},{"delay_ms":15000},{"remaining_ms":1000},
                       {"retry_after_ms":2000},{"attempt":3},{"max_retries":3},{"scope":"general"}):
            with self.subTest(change=change):
                events=recovered_retry();events[3][1].update(change)
                self.assertFalse(report(events)["diagnostic_complete"])
        self.assertFalse(report(recovered_retry(attempts=3))["diagnostic_complete"])
        events=recovered_retry(attempts=2);events[5][1].update(elapsed_ms=200,remaining_ms=14800)
        self.assertFalse(report(events)["diagnostic_complete"])

    def test_terminal_429_after_retry_remains_a_failed_download(self):
        events=recovered_retry();events[-3][1].update(status=429)
        events[-2][1].update(api_success=False,code=-6)
        result=report(events)
        self.assertTrue(result["diagnostic_complete"])
        self.assertFalse(result["http_success"])
        self.assertEqual(result["http_failures"],1)
        self.assertEqual(result["recovered_http_failures"],0)
        self.assertEqual(result["retried_http_failures"],1)
        self.assertEqual(result["http_failure_responses"],2)
    def test_kernel_explicit_default_ports_correlate_with_app_urls(self):
        events=log(redirect=True)
        for source,event in events:
            if source=="download-http":
                event["url"]=event["url"].replace(".test/", ".test:443/")
                if event["headers"].get("location"):
                    event["headers"]["location"]=event["headers"]["location"].replace(".test/", ".test:443/")
        self.assertTrue(report(events)["diagnostic_complete"])
        self.assertEqual(smoke.public_url("https://user:pass@[2001:db8::1]:443/a?key=value"), "https://[2001:db8::1]/a")
        self.assertEqual(smoke.public_url("http://[::1]:8080/a"), "http://[::1]:8080/a")
        self.assertEqual(smoke.public_url("http://example.test:80/a"), "http://example.test/a")

    def test_complete_http_success_and_redirect_chain(self):
        result = report(log(redirect=True))
        self.assertTrue(result["diagnostic_complete"])
        self.assertTrue(result["http_success"])
        self.assertEqual(len(result["cases"][0]["responses"]),2)

    def test_denials_are_complete_diagnostics_and_failed_downloads(self):
        for status in (403,429):
            result = report(log(status=status))
            self.assertTrue(result["diagnostic_complete"])
            self.assertFalse(result["http_success"])
            self.assertEqual(result["status"],"complete_with_http_failures")
            self.assertEqual(result["cases"][0]["responses"][0]["body_preview"],"denied")

    def test_image_decoder_rejection_does_not_turn_http_200_into_denial(self):
        result = report(log(route="image"))
        self.assertTrue(result["http_success"])
        self.assertEqual(result["cases"][0]["classification"],"http_succeeded_image_rejected")

    def test_missing_http_and_conflicting_app_duplicate_are_incomplete(self):
        self.assertFalse(report([event for event in log() if event[0]!="download-http"])["diagnostic_complete"])
        events=log(); duplicate=copy.deepcopy(events[-2]);duplicate[1]["code"]=-9;events.insert(-1,duplicate)
        self.assertFalse(report(events)["diagnostic_complete"])

    def test_newest_incomplete_run_never_uses_older_success(self):
        result=report(log("old")+log("new")[:2])
        self.assertEqual(result["run"],"new")
        self.assertFalse(result["diagnostic_complete"])

    def test_empty_log_and_wrong_case_matrix_never_pass(self):
        self.assertFalse(report([])["diagnostic_complete"])
        events=log();events[0][1]["urls"]=["https://other.test/a.png"]
        self.assertFalse(report(events)["diagnostic_complete"])

    def test_identical_app_duplicates_are_deduplicated(self):
        events=log();events += [event for event in events if event[0]=="download-smoke-app"]
        self.assertTrue(report(events)["diagnostic_complete"])

    def test_body_fingerprint_mismatch_cannot_pass(self):
        events=log();events[-2][1]["sha256"]="b"*64
        self.assertFalse(report(events)["diagnostic_complete"])

    def test_final_url_metadata_is_checked_before_counting_a_match(self):
        events=log(redirect=True);events[-2][1]["final_url"]="https://next.test/a.png"
        self.assertTrue(report(events)["diagnostic_complete"])
        events[-2][1]["final_url"]="https://example.test/a.png"
        self.assertFalse(report(events)["diagnostic_complete"])


if __name__ == "__main__":
    unittest.main()
