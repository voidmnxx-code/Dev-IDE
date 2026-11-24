package dev.ide.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import dev.ide.app.ui.IdeScreen
import dev.ide.app.ui.IdeTheme

class MainActivity : ComponentActivity() {
    private val vm: IdeViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent { IdeTheme { IdeScreen(vm) } }
    }

    override fun onPause() {
        super.onPause()
        vm.saveNow() // never lose edits when the app goes to the background
    }
}
