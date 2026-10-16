// Independent x64 Windows SDK ABI/constant witness for the Rust capsule.
#include <windows.h>
#include <psapi.h>
#include <cstddef>
#include <cstdio>

int main() {
    std::printf("{\"pointer_bytes\":%zu,\"extended_size\":%zu,\"extended_alignment\":%zu,"
        "\"basic_size\":%zu,\"basic_flags_offset\":%zu,\"basic_active_offset\":%zu,"
        "\"process_limit_offset\":%zu,\"job_limit_offset\":%zu,"
        "\"peak_process_offset\":%zu,\"peak_job_offset\":%zu,"
        "\"startup_ex_size\":%zu,\"process_counters_ex_size\":%zu,\"private_usage_offset\":%zu,"
        "\"basic_accounting_size\":%zu,\"completion_association_size\":%zu,"
        "\"flags\":%u,\"job_list\":%zu,\"create_flags\":%u,"
        "\"extended_class\":%u,\"accounting_class\":%u,\"pid_class\":%u,\"completion_class\":%u}\n",
        sizeof(void*), sizeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION), alignof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION),
        sizeof(JOBOBJECT_BASIC_LIMIT_INFORMATION), offsetof(JOBOBJECT_BASIC_LIMIT_INFORMATION, LimitFlags),
        offsetof(JOBOBJECT_BASIC_LIMIT_INFORMATION, ActiveProcessLimit),
        offsetof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION, ProcessMemoryLimit),
        offsetof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobMemoryLimit),
        offsetof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION, PeakProcessMemoryUsed),
        offsetof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION, PeakJobMemoryUsed),
        sizeof(STARTUPINFOEXW), sizeof(PROCESS_MEMORY_COUNTERS_EX), offsetof(PROCESS_MEMORY_COUNTERS_EX, PrivateUsage),
        sizeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION), sizeof(JOBOBJECT_ASSOCIATE_COMPLETION_PORT),
        unsigned(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_ACTIVE_PROCESS),
        size_t(PROC_THREAD_ATTRIBUTE_JOB_LIST),
        unsigned(EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT),
        unsigned(JobObjectExtendedLimitInformation), unsigned(JobObjectBasicAccountingInformation),
        unsigned(JobObjectBasicProcessIdList), unsigned(JobObjectAssociateCompletionPortInformation));
}
