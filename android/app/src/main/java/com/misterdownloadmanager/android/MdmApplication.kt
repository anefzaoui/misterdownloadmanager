package com.misterdownloadmanager.android

import android.app.Application
import com.misterdownloadmanager.android.core.Mdm
import com.misterdownloadmanager.android.service.DownloadService
import com.misterdownloadmanager.android.service.NetworkWatch
import com.misterdownloadmanager.android.service.Watcher

class MdmApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        DownloadService.channels(this)
        NetworkWatch.start(this)
        Watcher.start(this)
        // The core starts off the main thread: it loads the list and resumes what ran.
        Thread({ Mdm.start(this) }, "mdm-start").start()
    }
}
